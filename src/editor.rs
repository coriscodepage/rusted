use std::fmt::Display;

use crate::state::{ Address, EdError, Line };

pub struct Buffer {
    lines: Vec<String>,
    last_affected_line: usize,
    line_range: (usize, usize),
}

impl Buffer {
    pub fn new() -> Self {
        Self { lines: Vec::new(), last_affected_line: 0, line_range: (0, 0) }
    }

    pub fn set_mode(&mut self, address: &Address) {
        self.set_range(address);
        self.last_affected_line = self.line_range.1;
    }

    pub fn set_range(&mut self, address: &Address) {
        // println!("address: {:?}", address);
        self.line_range = self.parse_address(address);
        // println!("set range: {:?}", self.line_range);
    }

    pub fn append(&mut self, line: &str) {
        self.lines.insert(self.last_affected_line, line.to_owned());
        self.last_affected_line += 1;
        // println!("{:?}", self.lines);
    }

    pub fn delete(&mut self) -> Result<(), EdError> {
        let (from, to) = (
            self.line_range.0.checked_sub(1).ok_or(EdError::InvalidRange)?,
            self.line_range.1.checked_sub(1).ok_or(EdError::InvalidRange)?,
        );
        if to >= self.lines.len() {
            return Err(EdError::InvalidRange);
        }
        self.lines.drain(from..=to);
        self.last_affected_line = (from + 1).min(self.lines.len());
        Ok(())
    }

    fn parse_address(&self, address: &Address) -> (usize, usize) {
        match address {
            Address::None => (self.last_affected_line, self.last_affected_line),
            Address::Single(line) => (self.parse_line(line), self.parse_line(line)),
            Address::Range(line, line1) => (self.parse_line(line), self.parse_line(line1)),
        }
    }

    fn parse_line(&self, line: &Line) -> usize {
        match line {
            Line::Current => self.last_affected_line,
            Line::First => 1,
            Line::Last => self.lines.len(),
            Line::Absolute(n) => *n,
            Line::Offset(n) =>
                self.last_affected_line.saturating_add_signed(*n).min(self.lines.len()),
        }
    }

    pub fn get_lines(&mut self) -> Result<BufferView<'_>, EdError> {
        let (from, to) = (self.line_range.0.checked_sub(1).ok_or(EdError::InvalidRange)?, self.line_range.1.checked_sub(1).ok_or(EdError::InvalidRange)?);
        let lines = self.lines
            .get(from..=to)
            .ok_or(EdError::InvalidRange)?;
        self.last_affected_line = self.line_range.1;
        Ok(BufferView::new(lines, self.line_range.0))
    }

    pub fn get_active_line(&mut self) -> Result<BufferView<'_>, EdError> {
        let last = self.line_range.1;
        let line = self.lines.get(last - 1..=last - 1).ok_or(EdError::InvalidRange)?;
        self.last_affected_line = last;
        Ok(BufferView::new(line, self.lines.len()))
    }

    pub fn get_lines_for_save(&self, address: &Address) -> Result<String, EdError> {
        let address = if address == &Address::None {
            if self.lines.is_empty() {
                return Ok(String::new());
            }
            (1, self.lines.len())
        } else {
            self.parse_address(address)
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
        Self { lines, well_defined: false, numbered: false, range_start }
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
            write!(f, "{}", line.trim())?;
            if self.well_defined {
                writeln!(f, "$")?;
            } else {
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
        Self { lines, last_affected_line, line_range }
    }
}
