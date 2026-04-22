use std::fmt::Display;
use std::io::IsTerminal;

fn supports_color() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("PESTER_NO_COLOR").is_none()
        && std::env::var("TERM")
            .map(|term| term != "dumb")
            .unwrap_or(true)
}

fn paint(value: impl Display, code: &str) -> String {
    let value = value.to_string();
    if supports_color() {
        format!("\x1b[{code}m{value}\x1b[0m")
    } else {
        value
    }
}

pub fn bold(value: impl Display) -> String {
    paint(value, "1")
}

pub fn dim(value: impl Display) -> String {
    paint(value, "2")
}

pub fn green(value: impl Display) -> String {
    paint(value, "32")
}

pub fn yellow(value: impl Display) -> String {
    paint(value, "33")
}

pub fn blue(value: impl Display) -> String {
    paint(value, "34")
}

pub fn required_input(value: impl Display) -> String {
    paint(value, "1;33")
}

pub fn heading(value: impl Display) {
    println!("{}", bold(value));
}

pub fn detail(value: impl Display) {
    println!("  {}", dim(value));
}

pub fn key_value(key: impl Display, value: impl Display) {
    println!("  {} {}", dim(format!("{key}:")), value);
}

pub fn ok(value: impl Display) {
    println!("  {} {}", green("OK"), value);
}

pub fn warn(value: impl Display) {
    println!("  {} {}", yellow("WARN"), value);
}
