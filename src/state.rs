use std::{
    error::Error,
    fmt::{Debug, Display},
    io,
    iter::Peekable,
    str::Chars,
};

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
    RangeSemicolon(Line, Line),
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

pub trait MultiLineCommand: Debug {
    fn handle_line(&mut self, line: &str) -> Result<(), EdError>;
    fn is_done(&self) -> bool;
    fn finish(self: Box<Self>) -> Result<(CommandKind, Option<CommandKind>), EdError>;
}

#[derive(Debug)]
struct MLCSubstitution {
    re: Option<String>,
    sub: Vec<String>,
    flag: Option<String>,
    suffix: Option<CommandKind>,
    separator: char,
    is_done: bool,
}

impl MLCSubstitution {
    fn new(re: Option<String>, sub: String, separator: char) -> Self {
        let mut subs = Vec::new();
        subs.push(sub);
        Self {
            re,
            sub: subs,
            flag: None,
            suffix: None,
            separator,
            is_done: false,
        }
    }
}

impl MultiLineCommand for MLCSubstitution {
    fn handle_line(&mut self, line: &str) -> Result<(), EdError> {
        let mut parser = ParserInternal::new(line);
        let mut sub = String::new();
        self.is_done = true;
        while let Some(c) = parser.peek() {
            if c == '\\' {
                parser.consume();
                if let Some(v) = parser.peek() {
                    if v == '\n' {
                        self.is_done = false;
                        sub.push('\n');
                        continue;
                    }
                    sub.push(c);
                    continue;
                }
            } else if c == '/' || c.is_control() {
                break;
            }
            parser.consume();
            sub.push(c);
        }
        self.sub.push(sub);
        self.flag = parser
            .peek()
            .filter(|&c| c == self.separator)
            .map(|_| parser.consume())
            .and_then(|_| parser.peek())
            .and_then(|c| {
                if c.is_numeric() {
                    parser.parse_number().map(|n| format!("{}", n))
                } else if ['g'].contains(&c) {
                    parser.consume();
                    Some(String::from(c))
                } else {
                    Some("de".to_owned())
                }
            });

        if parser.peek().is_some_and(|v| !v.is_ascii_whitespace()) {
            self.suffix = Some(parser.parse_suffix()?);
        }
        Ok(())
    }

    fn is_done(&self) -> bool {
        self.is_done
    }

    fn finish(self: Box<Self>) -> Result<(CommandKind, Option<CommandKind>), EdError> {
        Ok((
            CommandKind::Substitution {
                re: self.re,
                sub: self.sub,
                flag: self.flag,
                repeated: false,
            },
            self.suffix,
        ))
    }
}

#[derive(Debug)]
pub enum CommandKind {
    NoOp,
    Quit,
    Append,
    Insert,
    Change,
    Yank,
    Delete,
    Transfer(Address),
    MultiLineCommand(Box<dyn MultiLineCommand>),
    Substitution {
        re: Option<String>,
        sub: Vec<String>,
        flag: Option<String>,
        repeated: bool,
    },
    Put,
    Join,
    Move(Address),
    Write(Option<String>),
    Edit(Option<String>),
    Read(Option<String>),
    File(Option<String>),
    List,
    NumberedList,
    PrintList,
    Undo,
    InternalListLastAffectedLine,
}

#[derive(Debug)]
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

pub struct Parser {
    mlc: Option<(Box<dyn MultiLineCommand>, Address)>,
}

impl Parser {
    pub fn new() -> Self {
        Self { mlc: None }
    }

    pub fn parse(&mut self, input: &str) -> Result<Command, EdError> {
        if let Some(mlc) = self.mlc.take() {
            return Ok(self.handle_mlc(mlc.0, mlc.1, input)?);
        }
        let mut parser = ParserInternal::new(input);
        let address = parser.parse_address()?;
        let (kind, suffix) = parser.parse_command()?;

        if matches!(kind, CommandKind::InternalListLastAffectedLine) && address == Address::None {
            Err(EdError::UnknownCommand)
        } else if let CommandKind::MultiLineCommand(cmd) = kind {
            self.mlc = Some((cmd, address));
            Ok(Command::empty(CommandKind::NoOp))
        } else {
            Ok(Command::new(address, kind, suffix))
        }
    }

    fn handle_mlc(
        &mut self,
        mut cmd: Box<dyn MultiLineCommand>,
        address: Address,
        line: &str,
    ) -> Result<Command, EdError> {
        cmd.handle_line(&line)?;
        if cmd.is_done() {
            let (kind, suffix) = cmd.finish()?;
            self.mlc = None;
            Ok(Command::new(address, kind, suffix))
        } else {
            self.mlc = Some((cmd, address));
            Ok(Command::empty(CommandKind::NoOp))
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

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.consume();
        }
    }

    fn parse_number(&mut self) -> Option<usize> {
        self.skip_whitespace();
        let mut s = String::new();
        while matches!(self.peek(), Some(d) if d.is_ascii_digit()) {
            s.push(self.consume().unwrap());
        }
        s.parse().ok()
    }

    fn parse_command(&mut self) -> Result<(CommandKind, Option<CommandKind>), EdError> {
        self.skip_whitespace();

        let cmd = self.consume().unwrap_or('\n');

        let candidate = match cmd {
            'q' => CommandKind::Quit,
            'a' => CommandKind::Append,
            'c' => CommandKind::Change,
            'i' => CommandKind::Insert,
            'm' => CommandKind::Move(self.parse_address()?),
            't' => CommandKind::Transfer(self.parse_address()?),
            'j' => CommandKind::Join,
            'y' => CommandKind::Yank,
            'x' => CommandKind::Put,
            'l' => CommandKind::List,
            'n' => CommandKind::NumberedList,
            'p' => CommandKind::PrintList,
            'd' => CommandKind::Delete,
            'u' => CommandKind::Undo,
            'w' => CommandKind::Write(self.parse_filename()?),
            'e' => CommandKind::Edit(self.parse_filename()?),
            'r' => CommandKind::Read(self.parse_filename()?),
            'f' => CommandKind::File(self.parse_filename()?),
            's' => {
                let mut working_copy = self.chars.clone();
                let (re, sub, flag) = match self.parse_full_subs() {
                    Ok(o) => match o {
                        (None, Some(v)) => v,
                        (Some(r), None) => return Ok((r, None)),
                        _ => return Err(EdError::UnexpectedState),
                    },
                    Err(_) => {
                        let ret = self.parse_short_subs(&mut working_copy)?;
                        while let Some(_) = self.consume() {}
                        return Ok((ret.0, ret.1));
                    }
                };
                CommandKind::Substitution {
                    re: re,
                    sub: vec![sub],
                    flag: flag,
                    repeated: false,
                }
            }
            c if c.is_ascii_whitespace() => CommandKind::InternalListLastAffectedLine,
            _ => {
                return Err(EdError::UnknownCommand);
            }
        };

        if self.peek().is_some_and(|v| !v.is_ascii_whitespace()) {
            let suffix = self.parse_suffix()?;
            Ok((candidate, Some(suffix)))
        } else {
            Ok((candidate, None))
        }
    }

    fn parse_short_subs(
        &self,
        chars: &mut Peekable<Chars>,
    ) -> Result<(CommandKind, Option<CommandKind>), EdError> {
        let mut flag = None;
        let mut suffix = None;

        match chars.peek().copied() {
            None => {}
            Some('\n') => {}
            Some(c) if c.is_ascii_whitespace() => return Err(EdError::InvalidInput),
            Some(c) if c.is_ascii_digit() => {
                let mut s = String::new();
                while matches!(chars.peek(), Some(d) if d.is_ascii_digit()) {
                    s.push(chars.next().unwrap());
                }
                flag = Some(s);
            }
            Some('g') => {
                chars.next();
                flag = Some("g".to_owned());
            }
            Some('l') | Some('n') | Some('p') => {
                suffix = Self::parse_print_suffix_char(chars.next().unwrap());
            }
            Some(_) => return Err(EdError::UnknownCommand),
        }

        if suffix.is_none() {
            suffix = match chars.peek().copied() {
                Some('l') | Some('n') | Some('p') => {
                    Self::parse_print_suffix_char(chars.next().unwrap())
                }
                _ => None,
            };
        }

        while matches!(chars.peek(), Some(c) if c.is_ascii_whitespace()) {
            chars.next();
        }

        if chars.peek().is_some() {
            return Err(EdError::UnknownCommand);
        }

        Ok((
            CommandKind::Substitution {
                re: None,
                sub: vec![],
                flag,
                repeated: true,
            },
            suffix,
        ))
    }

    fn parse_print_suffix_char(c: char) -> Option<CommandKind> {
        match c {
            'l' => Some(CommandKind::List),
            'n' => Some(CommandKind::NumberedList),
            'p' => Some(CommandKind::PrintList),
            _ => None,
        }
    }

    fn parse_full_subs(
        &mut self,
    ) -> Result<
        (
            Option<CommandKind>,
            Option<(Option<String>, String, Option<String>)>,
        ),
        EdError,
    > {
        let separator = self.consume().ok_or(EdError::EndOfInput)?;
        if separator.is_whitespace() {
            return Err(EdError::InvalidInput);
        }
        let mut re = String::new();
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.consume();
                if let Some(v) = self.peek() {
                    if v == separator {
                        self.consume();
                        re.push(v);
                        continue;
                    } else if v == '\\' {
                        self.consume();
                        re.push(v);
                        continue;
                    }
                }
            } else if c == separator {
                break;
            }
            self.consume();
            re.push(c);
        }
        if self.peek().is_none_or(|c| c != separator) {
            return Err(EdError::EndOfInput);
        } else {
            self.consume();
        }
        let re = if re.is_empty() { None } else { Some(re) };

        let mut sub = String::new();
        while let Some(c) = self.peek() {
            if c == '\\' {
                self.consume();
                if let Some(v) = self.peek() {
                    if v == '\n' {
                        sub.push('\n');
                        return Ok((
                            Some(CommandKind::MultiLineCommand(Box::new(
                                MLCSubstitution::new(re, sub, separator),
                            ))),
                            None,
                        ));
                    }
                    sub.push('\\');
                    continue;
                }
            } else if c == separator || c.is_control() {
                break;
            }
            self.consume();
            sub.push(c);
        }
        let flag = self
            .peek()
            .filter(|&c| c == separator)
            .map(|_| self.consume())
            .and_then(|_| self.peek())
            .and_then(|c| {
                if c.is_numeric() {
                    self.parse_number().map(|n| format!("{}", n))
                } else if ['g'].contains(&c) {
                    self.consume();
                    Some(String::from(c))
                } else {
                    Some("de".to_owned())
                }
            });
        Ok((None, Some((re, sub, flag))))
    }

    fn parse_suffix(&mut self) -> Result<CommandKind, EdError> {
        Self::parse_print_suffix_char(self.consume().ok_or(EdError::EndOfInput)?).ok_or(EdError::UnknownCommand)
    }

    fn parse_filename(&mut self) -> Result<Option<String>, EdError> {
        if self
            .peek()
            .is_some_and(|c| c.is_whitespace() || c.is_control())
        {
            while self.peek().is_some_and(|c| c.is_whitespace()) {
                self.consume();
            }
            let s = self.parse_rest();
            if s.is_empty() { Ok(None) } else { Ok(Some(s)) }
        } else {
            Err(EdError::SuffixUnsuported)
        }
    }

    fn parse_line(&mut self) -> Result<Option<Line>, EdError> {
        self.skip_whitespace();
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
            Some('+') | Some('-') => Ok(Some(Line::Offset(self.parse_offset()))),
            Some(d) if d.is_ascii_digit() => {
                let value = self.parse_number().unwrap();
                let offset = self.parse_offset();
                Ok(Some(Line::Absolute(value, offset)))
            }
            Some('/') => {
                self.consume();
                let mut regex = String::new();
                while let Some(c) = self.peek() {
                    match c {
                        '\\' => {
                            self.consume();
                            if self.peek().is_some_and(|ch| ch == '/') {
                                regex.push(self.consume().unwrap_or_default());
                            } else {
                                regex.push(c);
                            }
                        }
                        '/' => break,
                        _ => regex.push(self.consume().unwrap_or_default()),
                    };
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
                while let Some(c) = self.peek() {
                    match c {
                        '\\' => {
                            self.consume();
                            if self.peek().is_some_and(|ch| ch == '?') {
                                regex.push(self.consume().unwrap_or_default());
                            } else {
                                regex.push(c);
                            }
                        }
                        '?' => break,
                        _ => regex.push(self.consume().unwrap_or_default()),
                    };
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
            self.skip_whitespace();
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
        self.skip_whitespace();

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
                (Some(a), None) => Ok(Address::Range(a.clone(), a)),
                (None, Some(b)) => Ok(Address::Range(Line::First(0), b)),
                (None, None) => Ok(Address::Range(Line::First(0), Line::Last(0))),
            }
        } else if self.peek() == Some(';') {
            self.consume();
            let second = self.parse_line()?;
            match (first, second) {
                (Some(a), Some(b)) => Ok(Address::RangeSemicolon(a, b)),
                (Some(a), None) => Ok(Address::RangeSemicolon(a.clone(), a)),
                (None, Some(b)) => Ok(Address::RangeSemicolon(Line::Current(0), b)),
                (None, None) => Ok(Address::RangeSemicolon(Line::Current(0), Line::Last(0))),
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
    NoData,
    SuffixUnsuported,
    InvalidRange,
    InvalidAddress,
    InvalidFilename,
    InvalidInput,
    RegexNotFound,
    RegexInvalid,
    EndOfInput,
    UnexpectedState,
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
            EdError::NoData => "No Data".to_owned(),
            EdError::EndOfInput => "End Of Input".to_owned(),
            EdError::InvalidInput => "Invalid Input".to_owned(),
            EdError::UnexpectedState => "Unexpected State".to_owned(),
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
