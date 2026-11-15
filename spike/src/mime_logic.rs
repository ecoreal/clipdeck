//! 纯逻辑模块：MIME 类型分类与文本/图片 MIME 选择。
//!
//! 故意不依赖任何 Wayland / IO，方便单元测试（cargo test 无需图形会话）。

/// 剪贴板内容的粗分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// 可按 UTF-8 文本读取的内容。
    Text,
    /// 图片内容（image/*）。
    Image,
    /// 其它（如 application/x-kde-onlyReplaceEmpty 等 KDE 内部 MIME）。
    Other,
}

/// 将单个 MIME 字符串分类。大小写不敏感。
pub fn classify_mime(mime: &str) -> ContentKind {
    let mime = mime.to_ascii_lowercase();
    if mime.split(';').next().unwrap_or("").trim() == "text/plain" {
        return ContentKind::Text;
    }
    if mime.starts_with("text/") {
        // text/* 的其它变体（text/html、text/uri-list 等）也可按文本读
        return ContentKind::Text;
    }
    if mime.starts_with("image/") {
        return ContentKind::Image;
    }
    ContentKind::Other
}

/// 从一组 MIME 中选出最佳文本 MIME。
///
/// 优先级：`text/plain;charset=utf-8` > `text/plain` > 其余 `text/*`。
/// 没有任何文本 MIME 时返回 `None`。
pub fn pick_text_mime(mimes: &[&str]) -> Option<String> {
    if mimes.is_empty() {
        return None;
    }
    let mut best: Option<&str> = None;
    for &m in mimes {
        if classify_mime(m) != ContentKind::Text {
            continue;
        }
        best = Some(match best {
            None => m,
            Some(cur) => {
                let lm = m.to_ascii_lowercase();
                let lcur = cur.to_ascii_lowercase();
                if lm.starts_with("text/plain;charset=utf-8")
                    && !lcur.starts_with("text/plain;charset=utf-8")
                {
                    m
                } else if lm == "text/plain"
                    && !lcur.starts_with("text/plain;charset=utf-8")
                    && lcur != "text/plain"
                {
                    m
                } else {
                    cur
                }
            }
        });
    }
    best.map(str::to_string)
}

/// 从一组 MIME 中选出最佳图片 MIME。
///
/// 优先级：`image/png` > 其余 `image/*`。没有图片 MIME 时返回 `None`。
pub fn pick_image_mime(mimes: &[&str]) -> Option<String> {
    if mimes.is_empty() {
        return None;
    }
    let mut best: Option<&str> = None;
    for &m in mimes {
        if classify_mime(m) != ContentKind::Image {
            continue;
        }
        best = Some(match best {
            None => m,
            Some(cur) => {
                if m.eq_ignore_ascii_case("image/png") && !cur.eq_ignore_ascii_case("image/png") {
                    m
                } else {
                    cur
                }
            }
        });
    }
    best.map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_mime() {
        // 文本族
        assert_eq!(classify_mime("text/plain"), ContentKind::Text);
        assert_eq!(
            classify_mime("text/plain;charset=utf-8"),
            ContentKind::Text
        );
        assert_eq!(classify_mime("text/html"), ContentKind::Text);
        assert_eq!(classify_mime("TEXT/URI-LIST"), ContentKind::Text);
        // 图片族
        assert_eq!(classify_mime("image/png"), ContentKind::Image);
        assert_eq!(classify_mime("image/jpeg"), ContentKind::Image);
        // KDE 内部 / 未知
        assert_eq!(
            classify_mime("application/x-kde-onlyReplaceEmpty"),
            ContentKind::Other
        );
        assert_eq!(classify_mime(""), ContentKind::Other);
    }

    #[test]
    fn test_pick_text_mime_priority() {
        // utf-8 变体优先于裸 text/plain
        assert_eq!(
            pick_text_mime(&["text/plain", "text/plain;charset=utf-8"]).as_deref(),
            Some("text/plain;charset=utf-8")
        );
        // 只有裸 text/plain 时用它
        assert_eq!(
            pick_text_mime(&["text/plain", "text/html"]).as_deref(),
            Some("text/plain")
        );
        // 真实 KWin offer 场景：带 KDE 内部 MIME 也能挑出文本
        assert_eq!(
            pick_text_mime(&[
                "text/plain",
                "text/plain;charset=utf-8",
                "application/x-kde-onlyReplaceEmpty",
            ])
            .as_deref(),
            Some("text/plain;charset=utf-8")
        );
        // 无文本 MIME
        assert_eq!(pick_text_mime(&["image/png"]), None);
        assert_eq!(pick_text_mime(&[]), None);
    }

    #[test]
    fn test_pick_image_mime_priority() {
        // image/png 优先
        assert_eq!(
            pick_image_mime(&["image/jpeg", "image/png"]).as_deref(),
            Some("image/png")
        );
        // 无 png 时取第一个 image/*
        assert_eq!(
            pick_image_mime(&["image/jpeg"]).as_deref(),
            Some("image/jpeg")
        );
        // 纯文本 offer 中无图片
        assert_eq!(
            pick_image_mime(&["text/plain;charset=utf-8"]),
            None
        );
    }
}
