use std::fmt::Display;

use regex::Regex;

use crate::state::{Address, EdError, Line};

pub struct Buffer {
    lines: Vec<String>,
    last_affected_line: usize,
    line_range: (usize, usize),
    pub last_re: Option<String>,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            last_affected_line: 0,
            line_range: (0, 0),
            last_re: None,
        }
    }

    pub fn set_mode(&mut self, address: &Address) -> Result<(), EdError> {
        let address = self.parse_address(address)?;
        if address.0.max(address.1) > self.lines.len() {
            return Err(EdError::InvalidAddress);
        }
        self.line_range = address;
        self.last_affected_line = self.line_range.1;
        Ok(())
    }

    pub fn set_mode_insert(&mut self, address: &Address) -> Result<(), EdError> {
        let address = self.parse_address(address)?;
        if address.0.max(address.1) > self.lines.len() {
            return Err(EdError::InvalidAddress);
        }
        self.line_range = (address.0, address.1.saturating_sub(1));
        self.last_affected_line = self.line_range.1;
        Ok(())
    }

    pub fn set_range(&mut self, address: &Address) -> Result<(), EdError> {
        // println!("address: {:?}", address);
        self.line_range = self.parse_address(address)?;
        // println!("set range: {:?}", self.line_range);
        Ok(())
    }

    pub fn append(&mut self, line: &str) {
        self.lines.insert(self.last_affected_line, line.to_owned());
        self.last_affected_line += 1;
        // println!("{:?}", self.lines);
    }

    pub fn delete(&mut self) -> Result<Vec<String>, EdError> {
        let (from, to) = (
            self.line_range
                .0
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
            self.line_range
                .1
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
        );
        self.lines.get(from..=to).ok_or(EdError::InvalidRange)?;
        self.last_affected_line = (from + 1).min(self.lines.len());
        let yanked = self.lines.drain(from..=to);
        Ok(yanked.collect())
    }

    pub fn adj_change(&mut self) -> Result<(), EdError> {
        self.last_affected_line = self
            .line_range
            .0
            .checked_sub(1)
            .ok_or(EdError::InvalidRange)?;
        Ok(())
    }

    pub fn move_to(&mut self, address: &Address) -> Result<(), EdError> {
        let (from, to) = self.line_range;
        let destination = self.parse_address(address)?.1;
        if destination >= from && destination <= to {
            return Err(EdError::InvalidRange);
        }
        let yanked = self.delete()?;
        self.line_range = if destination > to {
            (destination - (to - from + 1), destination)
        } else {
            (destination, destination + (to - from))
        };
        // println!("set range: {:?}", self.line_range);
        self.last_affected_line = self.line_range.0;
        yanked.iter().for_each(|l| self.append(l));
        // println!("last affected: {}", self.last_affected_line);
        Ok(())
    }

    pub fn transfer(&mut self, address: &Address) -> Result<(), EdError> {
        let (from, to) = self.line_range;
        let destination = self.parse_address(address)?.1;
        if destination >= from && destination <= to {
            return Err(EdError::InvalidRange);
        }

        let yanked: Vec<String> = self.get_lines()?.into();
        self.set_mode(address)?;
        yanked.iter().for_each(|l| self.append(l));
        // println!("last affected: {}", self.last_affected_line);

        Ok(())
    }

    pub fn join(&mut self) -> Result<(), EdError> {
        let (from, to) = (
            self.line_range
                .0
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
            self.line_range
                .1
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
        );
        if from == to {
            return Ok(());
        }
        self.lines.get(from..=to).ok_or(EdError::InvalidRange)?;
        let lines = self.lines.drain(from + 1..=to).collect::<Vec<_>>();
        self.lines.get_mut(from).map(|v| {
            lines
                .iter()
                .for_each(|l| v.push_str(&l.trim_end_matches('\n')))
        });
        Ok(())
    }

    fn parse_address(&mut self, address: &Address) -> Result<(usize, usize), EdError> {
        match address {
            Address::None => Ok((self.last_affected_line, self.last_affected_line)),
            Address::Single(line) => Ok((self.parse_line(line)?, self.parse_line(line)?)),
            Address::Range(line, line1) => Ok((self.parse_line(line)?, self.parse_line(line1)?)),
            Address::RangeSemicolon(line, line1) => {
                let first = self.parse_line(line)?;
                let old = self.last_affected_line;
                self.last_affected_line = first;
                let second = self.parse_line(line1);
                self.last_affected_line = old;
                Ok((first, second?))
            }
        }
    }

    fn parse_line(&mut self, line: &Line) -> Result<usize, EdError> {
        match line {
            Line::Current(offset) => Ok(self
                .last_affected_line
                .checked_add_signed(*offset)
                .ok_or(EdError::InvalidAddress)?),
            Line::First(offset) => {
                if offset + 1 >= 1 {
                    Ok((*offset as usize) + 1)
                } else {
                    Err(EdError::InvalidAddress)
                }
            }
            Line::Last(offset) => self
                .lines
                .len()
                .checked_add_signed(*offset)
                .ok_or(EdError::InvalidAddress),
            Line::Absolute(value, offset) => value
                .checked_add_signed(*offset)
                .ok_or(EdError::InvalidAddress),
            Line::Offset(offset) => self
                .last_affected_line
                .checked_add_signed(*offset)
                .ok_or(EdError::InvalidAddress),
            Line::Regex(body, offset) => {
                let re = if body.is_empty() {
                    Regex::new(self.last_re.as_ref().ok_or(EdError::RegexInvalid)?)?
                } else {
                    self.last_re = Some(body.clone());
                    Regex::new(body)?
                };
                let index = self
                    .forward_search_space()
                    .find_map(|(address, line)| re.is_match(line).then_some(address));
                let idx = index
                    .map(|v| v.checked_add_signed(*offset).ok_or(EdError::InvalidAddress))
                    .transpose()?;
                // println!("{:?}", self.forward_search_space().collect::<Vec<_>>());
                // println!("{:?}", idx);
                Ok(idx.ok_or(EdError::RegexNotFound)?)
            }
            Line::RegexBackward(body, offset) => {
                let re = if body.is_empty() {
                    Regex::new(self.last_re.as_ref().ok_or(EdError::RegexInvalid)?)?
                } else {
                    self.last_re = Some(body.clone());
                    Regex::new(body)?
                };
                let index = self
                    .backward_search_space()
                    .find_map(|(address, line)| re.is_match(line).then_some(address));
                let idx = index
                    .map(|v| v.checked_add_signed(*offset).ok_or(EdError::InvalidAddress))
                    .transpose()?;
                // println!("{:?}", self.backward_search_space().collect::<Vec<_>>());
                // println!("{:?}", idx);
                Ok(idx.ok_or(EdError::RegexNotFound)?)
            }
        }
    }

    pub fn get_lines(&mut self) -> Result<BufferView<'_>, EdError> {
        let (from, to) = (
            self.line_range
                .0
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
            self.line_range
                .1
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
        );
        let lines = self.lines.get(from..=to).ok_or(EdError::InvalidRange)?;
        self.last_affected_line = self.line_range.1;
        Ok(BufferView::new(lines, self.line_range.0))
    }

    pub fn get_lines_mut(&mut self) -> Result<impl IntoIterator<Item = &mut String>, EdError> {
        let (from, to) = (
            self.line_range
                .0
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
            self.line_range
                .1
                .checked_sub(1)
                .ok_or(EdError::InvalidRange)?,
        );
        let lines = self.lines.get_mut(from..=to).ok_or(EdError::InvalidRange)?;
        self.last_affected_line = self.line_range.1;
        Ok(lines.iter_mut())
    }

    fn forward_search_space(&self) -> impl Iterator<Item = (usize, &String)> {
        let len = self.lines.len();
        let start = if len == 0 {
            0
        } else {
            self.last_affected_line % len
        };

        (0..len).map(move |offset| {
            let index = (start + offset) % len;
            (index + 1, &self.lines[index])
        })
    }

    fn backward_search_space(&self) -> impl Iterator<Item = (usize, &String)> {
        let len = self.lines.len();
        let start = self.last_affected_line.saturating_sub(1);

        (0..len).map(move |offset| {
            let index = (start + len - 1 - offset) % len;
            (index + 1, &self.lines[index])
        })
    }

    pub fn get_active_line(&mut self) -> Result<BufferView<'_>, EdError> {
        let last = self.line_range.1;
        let line = self
            .lines
            .get(last.saturating_sub(1)..=last.saturating_sub(1))
            .ok_or(EdError::InvalidRange)?;
        self.last_affected_line = last;
        Ok(BufferView::new(line, self.lines.len()))
    }

    pub fn get_lines_for_save(&mut self, address: &Address) -> Result<String, EdError> {
        let address = if address == &Address::None {
            if self.lines.is_empty() {
                return Ok(String::new());
            }
            (1, self.lines.len())
        } else {
            self.parse_address(address)?
        };
        let (from, to) = (
            address.0.checked_sub(1).ok_or(EdError::InvalidRange)?,
            address.1.checked_sub(1).ok_or(EdError::InvalidRange)?,
        );
        let mut f = String::new();
        for line in self.lines.get(from..=to).ok_or(EdError::InvalidRange)? {
            f.push_str(line);
        }
        Ok(f)
    }

    pub fn save_snapshot(&self) -> Snapshot {
        Snapshot::new(self.lines.clone(), self.last_affected_line, self.line_range)
    }

    pub fn restore_snapshot(&mut self, snapshot: Snapshot) {
        self.lines = snapshot.lines;
        self.line_range = snapshot.line_range;
        self.last_affected_line = snapshot.last_affected_line;
    }
}

pub struct BufferView<'a> {
    lines: &'a [String],
    well_defined: bool,
    numbered: bool,
    range_start: usize,
}

impl<'a> BufferView<'a> {
    fn new(lines: &'a [String], range_start: usize) -> Self {
        Self {
            lines,
            well_defined: false,
            numbered: false,
            range_start,
        }
    }

    pub fn well_defined(mut self) -> Self {
        self.well_defined = true;
        self
    }

    pub fn numbered(mut self) -> Self {
        self.numbered = true;
        self
    }
}

impl<'a> Display for BufferView<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, line) in self.lines.iter().enumerate() {
            if self.numbered {
                write!(f, "{}\t", self.range_start + i)?;
            }

            if self.well_defined {
                for c in line.chars() {
                    match c {
                        '\t' => write!(f, "\\t")?,
                        '\\' => write!(f, "\\\\")?,
                        '\n' => {}
                        c => write!(f, "{}", c)?,
                    }
                }
                writeln!(f, "$")?;
            } else {
                write!(f, "{}", line.trim())?;
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

impl<'a> Into<Vec<String>> for BufferView<'a> {
    fn into(self) -> Vec<String> {
        self.lines.to_vec()
    }
}

pub struct Snapshot {
    lines: Vec<String>,
    last_affected_line: usize,
    line_range: (usize, usize),
}

impl Snapshot {
    fn new(lines: Vec<String>, last_affected_line: usize, line_range: (usize, usize)) -> Self {
        Self {
            lines,
            last_affected_line,
            line_range,
        }
    }
}
