use std::{error::Error, fmt::Display, io, iter::Peekable, str::Chars};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFlow {
    Command,
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    None,
    Single(Line),
    Range(Line, Line),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Current(isize),
    First(isize),
    Last(isize),
    Absolute(usize, isize),
    Offset(isize),
    Regex(String, isize),
    RegexBackward(String, isize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandKind {
    Quit,
    Append,
    Write(Option<String>),
    Edit(String),
    List,
    NumberedList,
    Delete,
    Undo,
    InternalListLastAffectedLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub address: Address,
    pub kind: CommandKind,
    pub suffix: Option<CommandKind>,
}

impl Command {
    fn new(address: Address, kind: CommandKind, suffix: Option<CommandKind>) -> Self {
        Self {
            address,
            kind,
            suffix,
        }
    }

    pub fn empty(kind: CommandKind) -> Self {
        Self {
            address: Address::None,
            kind,
            suffix: None,
        }
    }
}

pub struct Parser {}

impl Parser {
    pub fn new() -> Self {
        Self {}
    }

    pub fn parse(&self, input: &str) -> Result<Command, EdError> {
        let mut parser = ParserInternal::new(input);
        let address = parser.parse_address()?;
        let (kind, suffix) = parser.parse_command()?;

        if kind == CommandKind::InternalListLastAffectedLine && address == Address::None {
            Err(EdError::UnknownCommand)
        } else {
            Ok(Command::new(address, kind, suffix))
        }
    }
}

struct ParserInternal<'a> {
    chars: Peekable<Chars<'a>>,
}

impl<'a> ParserInternal<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            chars: input.chars().peekable(),
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn consume(&mut self) -> Option<char> {
        self.chars.next()
    }

    fn parse_rest(&mut self) -> String {
        let mut r = String::new();
        while let Some(c) = self.consume() {
            if c == '\n' {
                break;
            }
            r.push(c);
        }
        r
    }

    fn parse_number(&mut self) -> Option<usize> {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.consume();
        }
        let mut s = String::new();
        while matches!(self.peek(), Some(d) if d.is_ascii_digit()) {
            s.push(self.consume().unwrap());
        }
        s.parse().ok()
    }

    fn parse_command(&mut self) -> Result<(CommandKind, Option<CommandKind>), EdError> {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.consume();
        }

        let cmd = self.consume().unwrap_or('\n');

        let candidate = match cmd {
            'q' => CommandKind::Quit,
            'a' => CommandKind::Append,
            'l' => CommandKind::List,
            'n' => CommandKind::NumberedList,
            'd' => CommandKind::Delete,
            'u' => CommandKind::Undo,
            'w' => CommandKind::Write({
                if self
                    .peek()
                    .is_some_and(|c| c.is_whitespace() || c.is_control())
                {
                    while self.peek().is_some_and(|c| c.is_whitespace()) {
                        self.consume();
                    }
                    let s = self.parse_rest();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    return Err(EdError::SuffixUnsuported);
                }
            }),
            'e' => CommandKind::Edit(if self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.parse_rest()
            } else {
                return Err(EdError::SuffixUnsuported);
            }),
            c if c.is_ascii_whitespace() => CommandKind::InternalListLastAffectedLine,
            _ => {
                return Err(EdError::UnknownCommand);
            }
        };

        if self.peek().is_some_and(|v| !v.is_ascii_whitespace()) {
            let suffix = match self.consume().unwrap() {
                'l' => CommandKind::List,
                'n' => todo!(),
                'p' => todo!(),
                _ => {
                    return Err(EdError::UnknownCommand);
                }
            };
            Ok((candidate, Some(suffix)))
        } else {
            Ok((candidate, None))
        }
    }

    fn parse_line(&mut self) -> Result<Option<Line>, EdError> {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.consume();
        }
        match self.peek() {
            Some('.') => {
                self.consume();
                let offset = self.parse_offset();
                Ok(Some(Line::Current(offset)))
            }
            Some('$') => {
                self.consume();
                let offset = self.parse_offset();
                Ok(Some(Line::Last(offset)))
            }
            Some('+') => Ok(Some(Line::Offset(self.parse_offset()))),
            Some('-') => Ok(Some(Line::Offset(self.parse_offset()))),
            Some(d) if d.is_ascii_digit() => {
                let value = self.parse_number().unwrap();
                let offset = self.parse_offset();
                Ok(Some(Line::Absolute(value, offset)))
            }
            Some('/') => {
                self.consume();
                let mut regex = String::new();
                while self.peek().is_some_and(|c| c != '/') {
                    regex.push(self.consume().unwrap_or_default());
                }
                if self.peek().is_some_and(|c| c == '/') {
                    self.consume();
                    let offset = self.parse_offset();
                    Ok(Some(Line::Regex(regex, offset)))
                } else {
                    Ok(Some(Line::Regex(regex, 0)))
                }
            }
            Some('?') => {
                self.consume();
                let mut regex = String::new();
                while self.peek().is_some_and(|c| c != '?') {
                    regex.push(self.consume().unwrap_or_default());
                }
                if self.peek().is_some_and(|c| c == '?') {
                    self.consume();
                    let offset = self.parse_offset();
                    Ok(Some(Line::RegexBackward(regex, offset)))
                } else {
                    Ok(Some(Line::RegexBackward(regex, 0)))
                }
            }
            _ => Ok(None),
        }
    }

    fn parse_offset(&mut self) -> isize {
        let mut result = 0;
        loop {
            while self.peek().is_some_and(|c| c.is_whitespace()) {
                self.consume();
            }
            match self.peek() {
                Some('+') => {
                    self.consume();
                    result += self.parse_number().map(|n| n as isize).unwrap_or(1);
                }
                Some('-') => {
                    self.consume();
                    result -= self.parse_number().map(|n| n as isize).unwrap_or(1);
                }
                Some(d) if d.is_numeric() => {
                    result += self.parse_number().unwrap_or(0) as isize;
                }
                _ => {
                    break;
                }
            }
        }
        result
    }

    fn parse_address(&mut self) -> Result<Address, EdError> {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.consume();
        }

        if self.peek() == Some('%') {
            self.consume();
            let offset = self.parse_offset();
            return Ok(Address::Range(Line::First(offset), Line::Last(offset)));
        }

        let first = self.parse_line()?;
        if self.peek() == Some(',') {
            self.consume();
            let second = self.parse_line()?;
            match (first, second) {
                (Some(a), Some(b)) => Ok(Address::Range(a, b)),
                (Some(a), None) => Ok(Address::Range(a, Line::Last(0))),
                (None, Some(b)) => Ok(Address::Range(Line::First(0), b)),
                (None, None) => Ok(Address::Range(Line::First(0), Line::Last(0))),
            }
        } else if self.peek() == Some(';') {
            self.consume();
            let second = self.parse_line()?;
            match (first, second) {
                (Some(a), Some(b)) => Ok(Address::Range(a, b)),
                (Some(a), None) => Ok(Address::Range(a.clone(), a)),
                (None, Some(b)) => Ok(Address::Range(Line::Current(0), b)),
                (None, None) => Ok(Address::Range(Line::Current(0), Line::Last(0))),
            }
        } else {
            match first {
                Some(a) => Ok(Address::Single(a)),
                None => Ok(Address::None),
            }
        }
    }
}

#[derive(Debug)]
pub enum EdError {
    UnknownCommand,
    SuffixUnsuported,
    InvalidRange,
    InvalidAddress,
    InvalidFilename,
    RegexNotFound,
    RegexInvalid,
    IoError(io::Error),
    RegexError(regex::Error),
}

impl Display for EdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            EdError::UnknownCommand => "Unknown Command".to_owned(),
            EdError::SuffixUnsuported => "Suffix Unsuported".to_owned(),
            EdError::InvalidRange => "Invalid Range".to_owned(),
            EdError::InvalidFilename => "Invalid Filename".to_owned(),
            EdError::IoError(error) => error.to_string(),
            EdError::RegexError(error) => error.to_string(),
            EdError::InvalidAddress => "Invalid Address".to_owned(),
            EdError::RegexNotFound => "Regex Not Found".to_owned(),
            EdError::RegexInvalid => "Regex Invalid".to_owned(),
        };
        write!(f, "{}", message)
    }
}

impl Error for EdError {}

impl From<io::Error> for EdError {
    fn from(value: io::Error) -> Self {
        Self::IoError(value)
    }
}

impl From<regex::Error> for EdError {
    fn from(value: regex::Error) -> Self {
        Self::RegexError(value)
    }
}
