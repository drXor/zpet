//! Command handling types

use regex::Regex;
use bot;
use zephyr;

pub enum Scope {
    Local, Everywhere, Secret,
}

pub struct Shape {
    patterns: Vec<Regex>,
}

pub struct CommandMatch<'a> {
    pub referent: &'a str,
    pub command: &'a str,
    pub args: Vec<&'a str>,
}

macro_rules! shape {
    [$($s:expr,)*] => {{
        lazy_static! {
            static ref PATTERNS: Vec<Regex> = vec![
                $( Regex::new($s).unwrap() ),*
            ];
        };
        Shape {
            patterns: PATTERNS.to_vec(),
        }
    }}
}

impl Shape {

    pub fn try_match<'a>(&self, expected_referent: &str, expected_labels: &Vec<&str>, val: &'a str) -> Option<CommandMatch<'a>> {
        for pattern in self.patterns.iter() {
            if let Some(caps) = pattern.captures(val) {
                let referent = caps.name("self").unwrap().as_str();
                if referent != expected_referent {
                    continue
                }
                let command  = caps.name("cmd").unwrap().as_str();
                if !expected_labels.contains(&command) {
                    continue
                }
                let mut args = vec![];
                let mut index = 0;
                loop {
                    if let Some(m) = caps.name(&format!("{}", index)) {
                        args.push(m.as_str());
                    } else {
                        break
                    }
                    index += 1;
                }
                return Some(CommandMatch {
                    referent, command, args
                })
            }
        }

        None
    }

    pub fn order() -> Shape {
        shape![
            "^(?P<self>[\\w]+) *, *(?P<cmd>[\\w]+) *[.!]?$", // topy, sit!
            "^(?P<cmd>[\\w]+) *, *(?P<self>[\\w]+) *[.!]?$", // sit, topy!
        ]
    }

    pub fn unary_order() -> Shape {
        shape![
            "^(?P<self>[\\w]+) *, *(?P<cmd>[\\w]+) +(?P<0>[\\w]+) *[.!]?$", // topy, get x!
            "^(?P<cmd>[\\w]+) +(?P<0>[\\w]+) *, *(?P<self>[\\w]+) *[.!]?$", // get x, topy!
        ]
    }

    pub fn invoke() -> Shape {
        shape![
            "^(?P<self>[\\w]+) *\\((?P<cmd>[ \\w]+)\\)$",     // topy(pet)
            "^(?P<cmd>[ \\w]+)s +(?P<self>[\\w]+)$",          // pets topy
            "^(?P<self>[\\w]+) *-> *\\{(?P<cmd>[ \\w]+)\\}$", // topy->{pet}
            "^(?P<self>[\\w]+) *(?:->|\\.|#|::) *(?P<cmd>[\\w]+)(?:\\(\\))?$", // topy.pet, topy.pet()
        ]
    }

    pub fn unary_invoke() -> Shape {
        shape![
            "^(?P<self>[\\w]+) *(?:->|\\.|#|::) *(?P<cmd>[\\w]+)(?:\\( *(?P<0>[\\w-.]+) *\\))?$",
            "^(?P<self>[\\w]+) *(?:->|\\.|#|::) *(?P<cmd>[\\w]+)(?:\\( *'(?P<0>[^']+)' *\\))?$",
        ]
    }

    pub fn binary_invoke() -> Shape {
        shape![
            "^(?P<self>[\\w]+) *(?:->|\\.|#|::) *(?P<cmd>[\\w]+)(?:\\( *(?P<0>[\\w-.]+) *, *(?P<1>[\\w-.]+) *\\))?$",
            "^(?P<self>[\\w]+) *(?:->|\\.|#|::) *(?P<cmd>[\\w]+)(?:\\( *'(?P<0>[^']+)' *, *'(?P<1>[^']+)' *\\))?$",
        ]
    }

    pub fn do_with() -> Shape {
        shape![
            "(?:^[\\w]+ +)?(?P<cmd>[\\w]+) +(?P<self>[\\w]+) +(?P<0>[ \\w]+)[.!]?$",
        ]
    }
}

pub struct Command {
    shape: Shape,
    scope: Scope,
    labels: Vec<String>,
    action: Box<Fn(&mut bot::State, &zephyr::Notice, &CommandMatch) -> ()>
}

impl Command {

    pub fn new<F>(shape: Shape, scope: Scope, labels: Vec<&str>, action: F) -> Command
        where F: Fn(&mut bot::State, &zephyr::Notice, &CommandMatch) -> () + 'static {

        Command {
            shape,
            scope,
            labels: labels.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
            action: Box::new(action)
        }
    }


    pub fn try_exec(&self, state: &mut bot::State, notice: &zephyr::Notice) -> bool {
        if let Some(cm) = self.shape.try_match(
            &state.name,
            &self.labels.iter().map(|x| x.as_ref()).collect::<Vec<_>>(),
            &notice.body.join("\n").trim()) {

            match self.scope {
                Scope::Local => if !(
                    state.class == notice.class &&
                        state.instance == notice.instance) {return false},
                _ => {},
            }

            (self.action)(state, notice, &cm);
            true
        } else {
            false
        }
    }
}

pub struct Handler {
    pub action: Box<Fn(&mut bot::State, &zephyr::Notice) -> bool>
}

impl Handler {

    pub fn new<F>(action: F) -> Handler
        where F: Fn(&mut bot::State, &zephyr::Notice) -> bool + 'static {
        Handler{ action: Box::new(action) }
    }

    pub fn try_exec(&self, state: &mut bot::State, notice: &zephyr::Notice) -> bool {
        (self.action)(state, notice)
    }
}

