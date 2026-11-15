//! clipdeck 可行性 spike：用 wl-clipboard-rs 常驻监听 Wayland 剪贴板变化，
//! 把每次变化打印到 stdout（文本内容 / 图片字节数）。
//!
//! 依赖 compositor 的 data-control 协议：KWin 6.3.6 暴露 zwlr_data_control_v1 v2，
//! wl-clipboard-rs 在有 ext-data-control-v1 时会自动优先使用（KDE 未来版本），
//! 当前则回落到 wlr-data-control。

mod mime_logic;

use std::io::Read;

use wl_clipboard_rs::paste::{ClipboardType, Seat};
use wl_clipboard_rs::watch::{ClipboardEvent, ClipboardType as WatchTarget, Watcher};

use crate::mime_logic::{pick_image_mime, pick_text_mime};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut watcher = Watcher::new(WatchTarget::Regular, Seat::Unspecified)?;
    eprintln!("clipdeck-spike: 已开始监听 regular clipboard（Ctrl-C 退出）");

    // 首次 next_event 立即报告当前剪贴板状态，之后阻塞等待变化。
    loop {
        match watcher.next_event()? {
            Some(ClipboardEvent::Changed {
                clipboard,
                mime_types,
                mut offer,
            }) => {
                let tag = match clipboard {
                    ClipboardType::Regular => "regular",
                    ClipboardType::Primary => "primary",
                };
                println!("[{tag}] changed, mimes = {mime_types:?}");
                let mimes: Vec<&str> = mime_types.iter().map(String::as_str).collect();

                if let Some(mime) = pick_text_mime(&mimes) {
                    let mut pipe = offer.receive(&mime)?;
                    let mut buf = Vec::new();
                    pipe.read_to_end(&mut buf)?;
                    println!("[{tag}] text ({mime}) = {:?}", String::from_utf8_lossy(&buf));
                } else if let Some(mime) = pick_image_mime(&mimes) {
                    let mut pipe = offer.receive(&mime)?;
                    let mut buf = Vec::new();
                    pipe.read_to_end(&mut buf)?;
                    let is_png = buf.starts_with(&[0x89, b'P', b'N', b'G']);
                    println!("[{tag}] image ({mime}) = {} bytes, png_magic={is_png}", buf.len());
                } else {
                    println!("[{tag}] 无文本/图片 MIME，跳过");
                }
            }
            Some(ClipboardEvent::Cleared { clipboard }) => {
                let tag = match clipboard {
                    ClipboardType::Regular => "regular",
                    ClipboardType::Primary => "primary",
                };
                println!("[{tag}] cleared");
            }
            None => {
                eprintln!("clipdeck-spike: watcher 已取消，退出");
                break;
            }
        }
    }
    Ok(())
}
