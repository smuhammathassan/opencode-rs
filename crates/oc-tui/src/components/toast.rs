//! Toast notifications.
//! From reference/packages/tui/src/ui/toast.tsx

use std::time::{Duration, Instant};

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastVariant {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub title: Option<String>,
    pub message: String,
    pub variant: ToastVariant,
    pub shown_at: Instant,
    pub duration: Duration,
}

impl Toast {
    pub fn new(message: impl Into<String>) -> Self {
        Toast {
            title: None,
            message: message.into(),
            variant: ToastVariant::Info,
            shown_at: Instant::now(),
            duration: Duration::from_secs(3),
        }
    }

    pub fn with_variant(mut self, variant: ToastVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn expired(&self) -> bool {
        self.shown_at.elapsed() > self.duration
    }

    pub fn variant_color(&self, theme: &Theme) -> ratatui::style::Color {
        match self.variant {
            ToastVariant::Info => theme.info,
            ToastVariant::Success => theme.success,
            ToastVariant::Warning => theme.warning,
            ToastVariant::Error => theme.error,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ToastStore {
    pub toasts: Vec<Toast>,
}

impl ToastStore {
    pub fn show(&mut self, toast: Toast) {
        self.toasts.retain(|t| !t.expired());
        self.toasts.push(toast);
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.show(Toast::new(message));
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.show(Toast::new(message).with_variant(ToastVariant::Error));
    }

    pub fn warning(&mut self, message: impl Into<String>) {
        self.show(Toast::new(message).with_variant(ToastVariant::Warning));
    }

    pub fn success(&mut self, message: impl Into<String>) {
        self.show(Toast::new(message).with_variant(ToastVariant::Success));
    }

    pub fn prune(&mut self) {
        self.toasts.retain(|t| !t.expired());
    }
}

/// Render the toast banner lines for the given terminal width.
pub fn toast_lines(store: &ToastStore, theme: &Theme) -> Vec<crate::components::text::StyledLine> {
    use crate::components::text::{pad_to, styled};
    store
        .toasts
        .iter()
        .map(|toast| {
            let color = toast.variant_color(theme);
            let text = match (&toast.title, toast.message.is_empty()) {
                (Some(title), false) => format!(" {title}: {} ", toast.message),
                (Some(title), true) => format!(" {title} "),
                _ => format!(" {} ", toast.message),
            };
            pad_to(styled(text, ratatui::style::Style::default().fg(color)), 1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_expiry() {
        let mut store = ToastStore::default();
        store.info("hello");
        assert_eq!(store.toasts.len(), 1);
        store.toasts[0].duration = Duration::from_millis(1);
        std::thread::sleep(Duration::from_millis(5));
        store.prune();
        assert!(store.toasts.is_empty());
    }

    #[test]
    fn caps_at_five() {
        let mut store = ToastStore::default();
        for i in 0..10 {
            store.info(format!("toast {i}"));
        }
        assert!(store.toasts.len() <= 5);
    }
}
