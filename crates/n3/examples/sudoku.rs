//! SPDX-FileCopyrightText: 2026 Blackcat Informatics® Inc. <paudley@blackcatinformatics.ca>
//! SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//!
//! 9x9 Sudoku backtracking solver (minimum-remaining-candidates search),
//! the same algorithm `purrdf-n3`'s `sudoku#solve` reasoner builtin wraps
//! (see `purrdf_n3::reasoner`, `SUDOKU_SOLVE`). The puzzle and solution are
//! both 81-character strings (`.` or `0` for a blank cell, `1`-`9` for a
//! given).
//!
//! This example is self-contained: it inlines the solver rather than calling
//! `crate::sudoku`, because the builtin's `solve_sudoku_string` entry point
//! is crate-private in `purrdf-n3`. Run it with:
//!
//! ```text
//! cargo run -p purrdf-n3 --example sudoku
//! ```

use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Solves a 9x9 Sudoku puzzle given as an 81-character string. Returns
/// `Ok(solution)` (an 81-character string of `1`-`9`) if solvable,
/// `Ok(String::new())` if the puzzle has no solution, or `Err(message)` if
/// the input is malformed: wrong length, a character that is not a digit or
/// `.`, or conflicting givens.
pub fn solve_sudoku_string(puzzle: &str) -> Result<String, String> {
    let text = puzzle.trim();
    if text.chars().count() != 81 || !text.chars().all(|c| c == '.' || c.is_ascii_digit()) {
        return Err(
            "sudoku expects an 81-character puzzle string containing digits or dots".to_string(),
        );
    }
    let mut cells = [0u8; 81];
    for (i, ch) in text.chars().enumerate() {
        cells[i] = if ch == '.' { 0 } else { ch as u8 - b'0' };
    }
    let peers = sudoku_peers();
    for (i, &value) in cells.iter().enumerate() {
        if value == 0 {
            continue;
        }
        if peers[i].iter().any(|&p| cells[p] == value) {
            return Err("sudoku puzzle has conflicting givens".to_string());
        }
    }
    Ok(if solve_sudoku_cells(&mut cells, peers) {
        cells.iter().map(|&v| char::from(b'0' + v)).collect()
    } else {
        String::new()
    })
}

fn solve_sudoku_cells(cells: &mut [u8; 81], peers: &'static [Vec<usize>; 81]) -> bool {
    let mut best_index = None;
    let mut best_candidates = Vec::new();
    for (i, &value) in cells.iter().enumerate() {
        if value != 0 {
            continue;
        }
        let candidates = sudoku_candidates(cells, &peers[i]);
        if candidates.is_empty() {
            return false;
        }
        if best_index.is_none() || candidates.len() < best_candidates.len() {
            best_index = Some(i);
            best_candidates = candidates;
            if best_candidates.len() == 1 {
                break;
            }
        }
    }
    let Some(index) = best_index else { return true };
    for value in best_candidates {
        cells[index] = value;
        if solve_sudoku_cells(cells, peers) {
            return true;
        }
        cells[index] = 0;
    }
    false
}

fn sudoku_candidates(cells: &[u8; 81], peers: &[usize]) -> Vec<u8> {
    let mut used = [false; 10];
    for &p in peers {
        let value = cells[p];
        if value != 0 {
            used[value as usize] = true;
        }
    }
    (1..=9u8).filter(|&v| !used[v as usize]).collect()
}

fn sudoku_peers() -> &'static [Vec<usize>; 81] {
    static PEERS: OnceLock<[Vec<usize>; 81]> = OnceLock::new();
    PEERS.get_or_init(|| {
        std::array::from_fn(|index| {
            let row = index / 9;
            let col = index % 9;
            let box_row = (row / 3) * 3;
            let box_col = (col / 3) * 3;
            let mut set = BTreeSet::new();
            for c in 0..9 {
                set.insert(row * 9 + c);
            }
            for r in 0..9 {
                set.insert(r * 9 + col);
            }
            for r in box_row..box_row + 3 {
                for c in box_col..box_col + 3 {
                    set.insert(r * 9 + c);
                }
            }
            set.remove(&index);
            set.into_iter().collect()
        })
    })
}

/// Renders an 81-character string as a 9x9 grid for display.
fn render_grid(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    (0..9)
        .map(|r| {
            let row: String = chars[r * 9..(r + 1) * 9].iter().collect();
            format!("{} {}\n", row, if r % 3 == 2 && r != 8 { "-" } else { " " })
        })
        .collect()
}

fn main() {
    let puzzle = "100007090030020008009600500005300900010080002600004000300000010040000007007000300";
    match solve_sudoku_string(puzzle) {
        Ok(solution) if solution.len() == 81 => {
            println!("puzzle:");
            print!("{}", render_grid(puzzle));
            println!("solution:");
            print!("{}", render_grid(&solution));
        }
        Ok(_) => println!("puzzle has no solution"),
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    }
}
