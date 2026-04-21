use std::io::{self, Write};

use anyhow::{bail, Result};

use crate::models::Config;

pub fn confirm_done(config: &Config, prompt: &str) -> Result<()> {
    match config.confirmation.done_phrase.as_deref() {
        Some(phrase) => {
            println!("{prompt}");
            println!("Type this phrase to confirm:");
            println!("{phrase}");
            let input = read_line("> ")?;
            if done_phrase_matches(&input, phrase) {
                Ok(())
            } else {
                bail!("confirmation phrase did not match")
            }
        }
        None => {
            if confirm_yes_no(prompt)? {
                Ok(())
            } else {
                bail!("cancelled")
            }
        }
    }
}

pub fn confirm_yes_no(prompt: &str) -> Result<bool> {
    println!("{prompt}");
    let input = read_line("Type yes or no: ")?;
    parse_yes_no(&input)
}

pub fn confirm_delete(prompt: &str) -> Result<()> {
    println!("{prompt}");
    let input = read_line("Type delete to continue: ")?;
    if input.trim() == "delete" {
        Ok(())
    } else {
        bail!("cancelled")
    }
}

pub fn read_phrase(prompt: &str) -> Result<String> {
    println!("{prompt}");
    read_line("> ")
}

fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim_end_matches(['\r', '\n']).to_string())
}

pub(crate) fn parse_yes_no(input: &str) -> Result<bool> {
    match input.trim().to_ascii_lowercase().as_str() {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => bail!("expected yes or no"),
    }
}

pub(crate) fn done_phrase_matches(input: &str, phrase: &str) -> bool {
    input.trim() == phrase
}

#[cfg(test)]
mod tests {
    use super::{done_phrase_matches, parse_yes_no};

    #[test]
    fn parses_full_yes_no_only() {
        assert!(parse_yes_no("yes").unwrap());
        assert!(parse_yes_no("YES").unwrap());
        assert!(!parse_yes_no("no").unwrap());
        assert!(!parse_yes_no("NO").unwrap());
        assert!(parse_yes_no("y").is_err());
        assert!(parse_yes_no("n").is_err());
        assert!(parse_yes_no("").is_err());
    }

    #[test]
    fn done_phrase_trims_input_but_requires_exact_phrase() {
        let phrase = "I am a lazy person who shouldn't cancel their reminders.";
        assert!(done_phrase_matches(phrase, phrase));
        assert!(done_phrase_matches(&format!("  {phrase}\n"), phrase));
        assert!(!done_phrase_matches("yes", phrase));
        assert!(!done_phrase_matches(
            "I am a lazy person who should not cancel their reminders.",
            phrase
        ));
    }
}
