//! 诗词包（pack）的格式定义、加载与校验。
//!
//! 一个诗词包是一个目录，里面有 `pack.toml`，以及它引用的故事文本和图层图片。
//! 内置诗词和用户自定义的诗词用同一种格式。

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 当前支持的包格式版本。
pub const FORMAT_VERSION: u32 = 1;

/// 包的清单文件名。
pub const MANIFEST: &str = "pack.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    /// 格式版本，用来兼容以后的字段变化。
    pub format: u32,
    pub poem: Poem,
    pub story: Option<Story>,
    #[serde(default, rename = "layer")]
    pub layers: Vec<Layer>,
    pub particles: Option<Particles>,
    #[serde(default)]
    pub text: TextStyle,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Poem {
    pub title: String,
    pub author: String,
    pub dynasty: String,
    /// 每个元素是一句。
    pub lines: Vec<String>,
    /// 出处，如「《全唐诗》卷三百六十八」。
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Story {
    /// 白话故事的 Markdown 文件，相对包目录。
    pub file: PathBuf,
    pub credibility: Credibility,
    /// 原典出处，如「《本事诗·情感》」。
    pub citation: Option<String>,
}

/// 故事的可信度：史书有载 / 笔记传说 / 后人推测。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Credibility {
    Recorded,
    Legend,
    Speculative,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    /// 图层图片，相对包目录。
    pub image: PathBuf,
    /// 视差深度，0 最远，1 最近。
    #[serde(default)]
    pub depth: f32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Particles {
    pub preset: ParticlePreset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticlePreset {
    Petals,
    Snow,
    Rain,
    Fireflies,
    MoonlightDust,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextStyle {
    #[serde(default)]
    pub position: TextPosition,
    #[serde(default)]
    pub orientation: Orientation,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextPosition {
    Left,
    Center,
    #[default]
    Right,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("{path}: 读取失败: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: 格式错误: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("{path}: {message}")]
    Invalid { path: PathBuf, message: String },
}

/// 已加载并通过校验的诗词包。
#[derive(Debug)]
pub struct LoadedPack {
    pub dir: PathBuf,
    pub pack: Pack,
}

impl LoadedPack {
    /// 读取 `dir/pack.toml`，解析并校验。
    pub fn load(dir: &Path) -> Result<Self, PackError> {
        let path = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&path).map_err(|source| PackError::Io {
            path: path.clone(),
            source,
        })?;
        let pack: Pack = toml::from_str(&text).map_err(|source| PackError::Parse {
            path: path.clone(),
            source,
        })?;
        validate(dir, &pack).map_err(|message| PackError::Invalid { path, message })?;
        Ok(Self {
            dir: dir.to_path_buf(),
            pack,
        })
    }
}

fn validate(dir: &Path, pack: &Pack) -> Result<(), String> {
    if pack.format != FORMAT_VERSION {
        return Err(format!(
            "不支持的格式版本 {}，当前只支持 {FORMAT_VERSION}",
            pack.format
        ));
    }
    if pack.poem.lines.iter().all(|l| l.trim().is_empty()) {
        return Err("poem.lines 不能为空".into());
    }
    if let Some(story) = &pack.story {
        require_file(dir, &story.file, "story.file")?;
    }
    for (i, layer) in pack.layers.iter().enumerate() {
        require_file(dir, &layer.image, &format!("layer[{i}].image"))?;
        if !(0.0..=1.0).contains(&layer.depth) {
            return Err(format!(
                "layer[{i}].depth 必须在 0 到 1 之间，现在是 {}",
                layer.depth
            ));
        }
    }
    Ok(())
}

fn require_file(dir: &Path, rel: &Path, field: &str) -> Result<(), String> {
    if rel.is_absolute() || rel.components().any(|c| c == std::path::Component::ParentDir) {
        return Err(format!("{field} 必须是包内的相对路径: {}", rel.display()));
    }
    if !dir.join(rel).is_file() {
        return Err(format!("{field} 指向的文件不存在: {}", rel.display()));
    }
    Ok(())
}

/// 加载 `root` 下每个子目录里的诗词包，返回成功的包和失败的错误。
pub fn load_all(root: &Path) -> Result<(Vec<LoadedPack>, Vec<PackError>), PackError> {
    let entries = std::fs::read_dir(root).map_err(|source| PackError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join(MANIFEST).is_file())
        .collect();
    dirs.sort();

    let mut packs = Vec::new();
    let mut errors = Vec::new();
    for dir in dirs {
        match LoadedPack::load(&dir) {
            Ok(p) => packs.push(p),
            Err(e) => errors.push(e),
        }
    }
    Ok((packs, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_pack(name: &str, manifest: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muse-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest).unwrap();
        dir
    }

    #[test]
    fn bundled_packs_are_valid() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
        let (packs, errors) = load_all(&root).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(!packs.is_empty());
    }

    #[test]
    fn rejects_unknown_format_version() {
        let dir = temp_pack(
            "version",
            "format = 2\n[poem]\ntitle='t'\nauthor='a'\ndynasty='d'\nlines=['x']\n",
        );
        let err = LoadedPack::load(&dir).unwrap_err().to_string();
        assert!(err.contains("不支持的格式版本 2"), "{err}");
    }

    #[test]
    fn rejects_missing_layer_image() {
        let dir = temp_pack(
            "layer",
            "format = 1\n[poem]\ntitle='t'\nauthor='a'\ndynasty='d'\nlines=['x']\n[[layer]]\nimage='sky.png'\n",
        );
        let err = LoadedPack::load(&dir).unwrap_err().to_string();
        assert!(err.contains("layer[0].image"), "{err}");
    }

    #[test]
    fn rejects_path_escaping_pack() {
        let dir = temp_pack(
            "escape",
            "format = 1\n[poem]\ntitle='t'\nauthor='a'\ndynasty='d'\nlines=['x']\n[story]\nfile='../x.md'\ncredibility='legend'\n",
        );
        let err = LoadedPack::load(&dir).unwrap_err().to_string();
        assert!(err.contains("相对路径"), "{err}");
    }

    #[test]
    fn rejects_unknown_field_with_line_number() {
        let dir = temp_pack(
            "unknown",
            "format = 1\n[poem]\ntitle='t'\nauthor='a'\ndynasty='d'\nlines=['x']\ncolour='red'\n",
        );
        let err = LoadedPack::load(&dir).unwrap_err().to_string();
        assert!(err.contains("colour") && err.contains("line 7"), "{err}");
    }
}
