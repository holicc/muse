//! 图层图片的解码缓存。
//!
//! 多个显示器的渲染器共用一份，同一张图只解码一次；切到下一首之前在后台线程预先解码，
//! 切换时就不会卡住事件循环。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

use image::RgbaImage;

/// 解码时的最大边长，更大的图按比例缩小。8K 以上对壁纸没有意义。
pub const MAX_DIMENSION: u32 = 8192;

type Decoded = Result<Arc<RgbaImage>, String>;

#[derive(Default)]
struct Inner {
    ready: HashMap<PathBuf, Decoded>,
    loading: HashSet<PathBuf>,
}

#[derive(Clone, Default)]
pub struct ImageCache(Arc<(Mutex<Inner>, Condvar)>);

impl ImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 取解码好的图；没有就当场解码（别的线程正在解码时等它）。
    pub fn get(&self, path: &Path) -> Decoded {
        let (lock, cv) = &*self.0;
        let mut inner = lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(r) = inner.ready.get(path) {
                return r.clone();
            }
            if !inner.loading.contains(path) {
                break;
            }
            inner = cv.wait(inner).unwrap_or_else(|e| e.into_inner());
        }
        inner.loading.insert(path.to_path_buf());
        drop(inner);

        // 解码中途 panic 时也要清掉 loading 标记并唤醒等待者，否则它们会永远卡住。
        struct Done<'a>(&'a ImageCache, &'a Path, Option<Decoded>);
        impl Drop for Done<'_> {
            fn drop(&mut self) {
                let (lock, cv) = &*self.0.0;
                let mut inner = lock.lock().unwrap_or_else(|e| e.into_inner());
                inner.loading.remove(self.1);
                let result = self
                    .2
                    .take()
                    .unwrap_or_else(|| Err(format!("{}: 解码时出错", self.1.display())));
                inner.ready.insert(self.1.to_path_buf(), result);
                cv.notify_all();
            }
        }
        let mut done = Done(self, path, None);
        let result = decode(path).map(Arc::new);
        done.2 = Some(result.clone());
        result
    }

    /// 在后台线程解码这些图。
    pub fn prefetch(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let cache = self.clone();
        let _ = std::thread::Builder::new()
            .name("muse-prefetch".into())
            .spawn(move || {
                for p in paths {
                    let _ = cache.get(&p);
                }
            });
    }

    /// 放进一张已经读到内存里的图，`path` 只作为索引；给读不了磁盘的环境（网页）用。
    pub fn insert_encoded(&self, path: PathBuf, bytes: &[u8]) -> Result<(), String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("{}: 图片加载失败: {e}", path.display()))?;
        let (lock, cv) = &*self.0;
        let mut inner = lock.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .ready
            .insert(path, Ok(Arc::new(shrink(img).to_rgba8())));
        cv.notify_all();
        Ok(())
    }

    /// 这张图已经解码好了吗。
    pub fn contains(&self, path: &Path) -> bool {
        let (lock, _) = &*self.0;
        lock.lock()
            .unwrap_or_else(|e| e.into_inner())
            .ready
            .get(path)
            .is_some_and(|r| r.is_ok())
    }

    /// 只保留这些图，其余释放内存。
    pub fn retain(&self, keep: &[PathBuf]) {
        let (lock, _) = &*self.0;
        let mut inner = lock.lock().unwrap_or_else(|e| e.into_inner());
        inner.ready.retain(|p, _| keep.contains(p));
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.0.lock().unwrap().ready.len()
    }
}

fn decode(path: &Path) -> Result<RgbaImage, String> {
    // 按内容判断格式：Windows 的 TranscodedWallpaper 没有扩展名。
    let img = image::ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("{}: 图片读取失败: {e}", path.display()))?
        .decode()
        .map_err(|e| format!("{}: 图片加载失败: {e}", path.display()))?;
    Ok(shrink(img).to_rgba8())
}

fn shrink(img: image::DynamicImage) -> image::DynamicImage {
    if img.width() > MAX_DIMENSION || img.height() > MAX_DIMENSION {
        img.resize(
            MAX_DIMENSION,
            MAX_DIMENSION,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_once_and_evicts() {
        let dir = std::env::temp_dir().join(format!("muse-images-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.png");
        RgbaImage::new(4, 2).save(&a).unwrap();
        let cache = ImageCache::new();
        cache.prefetch(vec![a.clone()]);
        let x = cache.get(&a).unwrap();
        let y = cache.get(&a).unwrap();
        assert!(Arc::ptr_eq(&x, &y));
        assert_eq!(x.dimensions(), (4, 2));
        assert!(cache.get(&dir.join("missing.png")).is_err());
        cache.retain(std::slice::from_ref(&a));
        assert_eq!(cache.len(), 1);
        cache.retain(&[]);
        assert_eq!(cache.len(), 0);
    }
}
