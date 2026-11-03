//! Markdown in an IntUI Text as runs: the words a person reads, with the bold and italic ones marked.
//! `**bold**`, `*italic*` and `_italic_` are marks only when they close; an unclosed one is kept as the
//! character it is. `_` marks only at a word's edge, so `usage_history` stays one word. A backslash before
//! punctuation stops a mark (`usage\_history`), heading marks and code ticks are dropped, and nothing else
//! of Markdown is drawn.

use crate::ui::Run;

/// A paragraph of more runs than this is drawn in one face: shaping each run costs time, and text from another agent could be all marks.
const MAX_RUNS: usize = 500;

/// The paragraphs of `markdown`, each a list of runs; lines of one paragraph are joined by a newline.
pub fn paragraphs(markdown: &str) -> Vec<Vec<Run>> {
    markdown
        .split("\n\n")
        .map(|para| {
            let mut runs: Vec<Run> = vec![];
            for (n, line) in para.lines().map(|l| l.trim_start_matches('#').trim()).enumerate() {
                if n > 0 {
                    push(&mut runs, "\n", false, false);
                }
                for r in inline(line) {
                    push(&mut runs, &r.text, r.bold, r.italic);
                }
            }
            if runs.len() > MAX_RUNS {
                let text = runs.iter().map(|r| r.text.as_str()).collect();
                runs = vec![Run { text, bold: false, italic: false }];
            }
            runs
        })
        .filter(|runs| runs.iter().any(|r| !r.text.trim().is_empty()))
        .collect()
}

fn push(runs: &mut Vec<Run>, text: &str, bold: bool, italic: bool) {
    match runs.last_mut() {
        Some(last) if last.bold == bold && last.italic == italic => last.text.push_str(text),
        _ => runs.push(Run { text: text.to_string(), bold, italic }),
    }
}

fn inline(line: &str) -> Vec<Run> {
    let c: Vec<char> = line.chars().collect();
    let word = |i: Option<usize>| i.and_then(|i| c.get(i)).is_some_and(|ch| ch.is_alphanumeric());
    let closes = |from: usize, mark: &[char]| (from..c.len()).any(|j| c[j..].starts_with(mark));
    let (mut runs, mut bold, mut italic, mut i) = (vec![], false, false, 0);
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && c.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) {
            push(&mut runs, &c[i + 1].to_string(), bold, italic);
            i += 2;
        } else if ch == '*' && c.get(i + 1) == Some(&'*') && (bold || closes(i + 2, &['*', '*'])) {
            bold = !bold;
            i += 2;
        } else if ch == '*' && c.get(i + 1) != Some(&'*') && (italic || closes(i + 1, &['*'])) {
            italic = !italic;
            i += 1;
        } else if ch == '_' && ((italic && !word(Some(i + 1))) || (!italic && !word(i.checked_sub(1)) && c.get(i + 1).is_some_and(|n| !n.is_whitespace()) && closes(i + 1, &['_']))) {
            italic = !italic;
            i += 1;
        } else if ch == '`' {
            i += 1;
        } else {
            push(&mut runs, &ch.to_string(), bold, italic);
            i += 1;
        }
    }
    runs
}
