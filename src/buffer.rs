use std::fmt::Display;

use regex::Regex;

use crate::state::{Address, EdError, Line};

pub struct Buffer {
    lines: Vec<String>,
    last_affected_line: usize,
    line_range: (usize, usize),
    last_bre: Option<String>,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            last_affected_line: 0,
            line_range: (0, 0),
            last_bre: None,
        }
    }

    pub fn set_mode(&mut self, address: &Address) -> Result<(), EdError> {
        self.set_range(address)?;
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

    pub fn delete(&mut self) -> Result<(), EdError> {
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
        if to >= self.lines.len() {
            return Err(EdError::InvalidRange);
        }
        self.lines.drain(from..=to);
        self.last_affected_line = (from + 1).min(self.lines.len());
        Ok(())
    }

    fn parse_address(&mut self, address: &Address) -> Result<(usize, usize), EdError> {
        match address {
            Address::None => Ok((self.last_affected_line, self.last_affected_line)),
            Address::Single(line) => Ok((self.parse_line(line)?, self.parse_line(line)?)),
            Address::Range(line, line1) => Ok((self.parse_line(line)?, self.parse_line(line1)?)),
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
                    Regex::new(self.last_bre.as_ref().ok_or(EdError::RegexInvalid)?)?
                } else {
                    self.last_bre = Some(body.clone());
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
                    Regex::new(self.last_bre.as_ref().ok_or(EdError::RegexInvalid)?)?
                } else {
                    self.last_bre = Some(body.clone());
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
            .get(last - 1..=last - 1)
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
                        '\n' => {},
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
