//! Basic facilities for sending and receiving Zephyr notices

use std::result::{Result as SResult};
use std::fmt::{Formatter, Display, Error};
use std::io::{Read, Write, Result, BufReader};
use std::process::*;
use std::time::Duration;

use std::mem;

use tempfile::NamedTempFile;

use regex::Regex;

/// Enum representing a notice direction
#[derive(Clone, Debug)]
pub enum Direction {
    Incoming, Outgoing
}

/// Struct representing a Zephyr notice
#[derive(Clone, Debug)]
pub struct Notice {
    pub opcode:    String,
    pub direction: Direction,
    pub class:     String,
    pub instance:  String,
    pub sender:    String,
    pub zsig:      String,
    pub body:      Vec<String>,

    pub incoming_data: Option<IncomingData>,
}

/// Data unique to an incoming zephyrgram
#[derive(Clone, Debug)]
pub struct IncomingData {
    pub is_auth:   bool,
    pub date:      Duration,
    pub host:      String,
}

impl Notice {

    pub fn new_outgoing(
        opcode:   &str,
        class:    &str,
        instance: &str,
        sender:   &str,
        zsig:     &str,
        body:     &str,
    ) -> Notice {
        Notice::new_outgoing_with_wrap(opcode, class, instance, sender, zsig, body, 70)
    }

    pub fn new_outgoing_with_wrap(
        opcode:   &str,
        class:    &str,
        instance: &str,
        sender:   &str,
        zsig:     &str,
        body:     &str,
        wrap:     usize
    ) -> Notice {

        Notice {
            opcode:    opcode.to_string(),
            direction: Direction::Outgoing,
            class:     class.to_string(),
            instance:  instance.to_string(),
            sender:    sender.to_string(),
            zsig:      zsig.to_string(),
            body:      wrap_lines(wrap, body),

            incoming_data: None,
        }
    }

    pub fn make_reply(&self, sender: &str, zsig: &str, body: &str) -> Notice {
        self.triplet().make_reply(sender, zsig, body)
    }

    pub fn triplet(&self) -> Triplet {
        Triplet::of_instance(&self.class, &self.instance)
    }

    pub fn was_sent_to(&self, triplet: &Triplet) -> bool {
        triplet.class == self.class &&
            (triplet.instance.is_none() ||
                triplet.instance.as_ref().unwrap() == &self.instance)
    }

    pub fn is_auth(&self) -> bool {
        match self.incoming_data {
            Some(ref data) => data.is_auth,
            None => false,
        }
    }
}

/// Struct representing a Zephyr triplet
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Triplet {
    pub class: String,
    pub instance: Option<String>,
    pub recipient: Option<String>,
}

impl Triplet {

    pub fn of_class(class: &str) -> Triplet {
        Triplet {
            class: class.to_string(),
            instance: None,
            recipient: None,
        }
    }

    pub fn of_instance(class: &str, instance: &str) -> Triplet {
        Triplet {
            class: class.to_string(),
            instance: Some(instance.to_string()),
            recipient: None,
        }
    }

    pub fn new(class: &str, instance: &str, recipient: &str) -> Triplet {
        Triplet {
            class: class.to_string(),
            instance: Some(instance.to_string()),
            recipient: Some(recipient.to_string()),
        }
    }

    pub fn make_reply(&self, sender: &str, zsig: &str, body: &str) -> Notice {
        Notice::new_outgoing("AUTO", &self.class,
                             &self.instance.as_ref().unwrap_or(&"personal".to_string()),
                             sender, zsig, body)
    }
}

impl Display for Triplet {

    fn fmt(&self, f: &mut Formatter) -> SResult<(), Error> {

        let instance = self.instance.as_ref().map(|x| x.as_ref()).unwrap_or("*");
        let recipient = self.recipient.as_ref().map(|x| x.as_ref()).unwrap_or("*");
        write!(f, "{},{},{}", self.class, instance, recipient)
    }
}

/// Struct wrapping an extrenal zwgc process,
/// and access to zwrite
pub struct Zephyr {
    subs: Vec<Triplet>,
    format_file: Option<NamedTempFile>,
    sub_file: Option<NamedTempFile>,
    child: Option<Child>,
}

impl Zephyr {

    pub fn new(subs: Vec<Triplet>) -> Result<Zephyr> {

        let mut format_file = NamedTempFile::new().expect("failed to create temporary file");
        let mut sub_file = NamedTempFile::new().expect("failed to create temporary file");

        write!(format_file, "{}", FORMAT)?;

        for sub in subs.iter() {
            write!(sub_file, "{}\n", sub)?;
        }

        let mut zio = Zephyr { subs, format_file: Some(format_file), sub_file: Some(sub_file), child: None };
        zio.restart()?;

        // read the first message and discard it
        zio.read()?;

        eprintln!("connected to zephyr!");

        Ok(zio)
    }

    pub fn subs(&self) -> &Vec<Triplet> {
        &self.subs
    }

    pub fn restart(&mut self) -> Result<()> {
        self.kill()?;

        let child = Command::new("zwgc")
            .arg("-nofork")
            .arg("-ttymode")
            .arg("-f")
            .arg(format!("{}", self.format_file.as_ref().unwrap().path().to_str().unwrap()))
            .arg("-subfile")
            .arg(format!("{}", self.sub_file.as_ref().unwrap().path().to_str().unwrap()))
            .stdout(Stdio::piped())
            .spawn()?;

        self.child = Some(child);
        Ok(())
    }

    pub fn kill(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child.as_mut() {
            child.kill()?;
        }
        self.child = None;
        Ok(())
    }

    pub fn read_raw(&mut self) -> Result<String> {
        let child = self.child.as_mut().unwrap();
        let out = child.stdout.as_mut().unwrap();

        let mut bytes = vec![];
        let mut buffer = [0; 512];

        let mut reader = BufReader::new(out);

        loop {
            let len = reader.read(&mut buffer)?;
            bytes.extend_from_slice(&buffer[..len]);
            if len < buffer.len() {
                break;
            }
        }

        Ok(String::from_utf8(bytes).unwrap())
    }

    pub fn read(&mut self) -> Result<Notice> {
        let raw = self.read_raw()?;

        let mut opcode   = String::new();
        let mut class    = String::new();
        let mut instance = String::new();
        let mut sender   = String::new();
        let mut auth     = String::new();
        let mut time     = String::new();
        let mut date     = String::new();
        let mut host     = String::new();
        let mut zsig     = String::new();
        let mut body     = Vec::new();

        for line in raw.split('\n') {
            let split = line.splitn(2, ": ").collect::<Vec<_>>();
            match split[0] {
                "opcode"    => opcode   += split[1],
                "class"     => class    += split[1],
                "instance"  => instance += split[1],
                "sender"    => sender   += split[1],
                "auth"      => auth     += split[1],
                "time"      => time     += split[1],
                "date"      => date     += split[1],
                "fromhost"  => host     += split[1],
                "signature" => zsig     += split[1],
                "body"      => body.push(split[1].to_string()),
                _ => {}
            }
        }

        let incoming_data = Some(IncomingData {
            is_auth: auth == "yes",
            date: Duration::from_millis(0), // FIXME
            host,
        });

        let notice = Notice {
            opcode,
            direction: Direction::Incoming,
            class,
            instance,
            sender,
            zsig,
            body,

            incoming_data,
        };

        Ok(notice)
    }

    // NB: self is &mut for future-proofing
    pub fn zwrite(&mut self, notice: &Notice) -> Result<()> {

        let mut body = String::new();
        for line in notice.body.iter() {
            body += format!("{}\n", line).as_str();
        }

        let mut child = Command::new("zwrite")
            .arg("-d")
            .arg("-c").arg(notice.class.as_str())
            .arg("-i").arg(notice.instance.as_str())
            .arg("-S").arg(notice.sender.as_str())
            .arg("-s").arg(notice.zsig.as_str())
            .arg("-O").arg(notice.opcode.as_str())
            .arg("-m").arg(body)
            .spawn()?;

        child.wait()?;

        Ok(())
    }

}

impl Drop for Zephyr {
    fn drop(&mut self) {
        self.kill().expect("failed to destroy process");

        let mut format_file = None;
        let mut sub_file = None;

        mem::swap(&mut self.format_file, &mut format_file);
        mem::swap(&mut self.sub_file, &mut sub_file);

        format_file.unwrap().close().expect("failed to destroy temp file");
        sub_file.unwrap().close().expect("failed to destroy temp file");
    }
}

fn wrap_lines(limit: usize, val: &str) -> Vec<String> {
    lazy_static! {
        static ref PATTERN: Regex = Regex::new("[ \0]").unwrap();
    }

    let mut buf = String::new();
    let mut line_len = 0;

    for word in PATTERN.split(&val.replace("\n", "\n\0")) {
        if line_len + word.len() > limit {
            buf += "\n";
            buf += word;
            line_len = 0;
        } else if line_len == 0 {
            buf += word;
        } else {
            buf += " ";
            buf += word;
            line_len += 1;
        }
        if word.ends_with("\n") {
            line_len = 0;
        } else {
            line_len += word.len();
        }
    }

    buf.split("\n").map(|x| x.to_string()).collect::<Vec<_>>()
}

// ZWGC format file
const FORMAT: &str = r#"
if (downcase($opcode) == "ping") then

	exit
endif

case downcase($class)
match "mail"
	exit

default
	fields signature body
	if (downcase($recipient) == downcase($user)) then
		print "personal\n"
	endif
	while ($opcode != "") do
		print "opcode:" lbreak($opcode, "\n")
		print "\n"
		set dummy = lany($opcode, "\n")
	endwhile
	while ($class != "") do
		print "class:" lbreak($class, "\n")
		print "\n"
		set dummy = lany($class, "\n")
	endwhile
	while ($instance != "") do
		print "instance:" lbreak($instance, "\n")
		print "\n"
		set dummy = lany($instance, "\n")
	endwhile
	while ($sender != "") do
		print "sender:" lbreak($sender, "\n")
		print "\n"
		set dummy = lany($sender, "\n")
	endwhile
	while ($auth != "") do
		print "auth:" lbreak($auth, "\n")
		print "\n"
		set dummy = lany($auth, "\n")
	endwhile
	while ($time != "") do
		print "time:" lbreak($time, "\n")
		print "\n"
		set dummy = lany($time, "\n")
	endwhile
	while ($date != "") do
		print "date:" lbreak($date, "\n")
		print "\n"
		set dummy = lany($date, "\n")
	endwhile
	while ($fromhost != "") do
		print "fromhost:" lbreak($fromhost, "\n")
		print "\n"
		set dummy = lany($fromhost, "\n")
	endwhile
	while ($signature != "") do
		print "signature:" lbreak($signature, "\n")
		print "\n"
		set dummy = lany($signature, "\n")
	endwhile
	while ($body != "") do
		print "body:" lbreak($body, "\n")
		print " \n"
		set dummy = lany($body, "\n")
	endwhile
	print "done\n"
	put "stdout"
	exit

endcase
"#;
