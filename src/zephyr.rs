//! Basic facilities for sending and receiving Zephyr notices

use std::result::{Result as SResult};
use std::fmt::{Formatter, Display, Error};
use std::io::{Read, Write, Result, BufReader};
use std::process::*;
use std::time::Duration;

use tempfile::NamedTempFile;

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
        body:     Vec<&str>,
    ) -> Notice {

        Notice {
            opcode:    opcode.to_string(),
            direction: Direction::Outgoing,
            class:     class.to_string(),
            instance:  instance.to_string(),
            sender:    sender.to_string(),
            zsig:      zsig.to_string(),
            body:      body.iter().map(|s| s.to_string()).collect(),

            incoming_data: None,
        }
    }

    pub fn make_reply(&self, sender: &str, zsig: &str, body: Vec<&str>) -> Notice {

        Notice {
            opcode: "AUTO".to_string(),
            direction: Direction::Outgoing,
            class: self.class.clone(),
            instance: self.instance.clone(),
            sender: sender.to_string(),
            zsig: zsig.to_string(),
            body: body.iter().map(|s| s.to_string()).collect(),
            incoming_data: None,
        }
    }
}

/// Struct representing a Zephyr triplet
#[derive(Clone, Debug)]
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
}

impl Display for Triplet {

    fn fmt(&self, f: &mut Formatter) -> SResult<(), Error> {

        let instance = self.instance.as_ref().map(|x| x.as_ref()).unwrap_or("*");
        let recipient = self.recipient.as_ref().map(|x| x.as_ref()).unwrap_or("*");
        write!(f, "{},{},{}", self.class, instance, recipient)
    }
}

/// Struct wrapping an extrenal zwgc process
pub struct Zephyr {
    format_file: NamedTempFile,
    sub_file: NamedTempFile,
    child: Option<Child>,
}

impl Zephyr {

    pub fn new(subs: Vec<Triplet>) -> Result<Zephyr> {

        let mut format_file = NamedTempFile::new().expect("failed to create temporary file");
        let mut sub_file = NamedTempFile::new().expect("failed to create temporary file");

        write!(format_file, "{}", FORMAT)?;

        for sub in subs.iter() {
            write!(sub_file, "{}", sub)?;
        }

        let mut zio = Zephyr { format_file, sub_file, child: None };
        zio.restart()?;

        // read the first message and discard it
        zio.read()?;

        Ok(zio)
    }

    pub fn restart(&mut self) -> Result<()> {
        self.kill()?;

        let child = Command::new("zwgc")
            .arg("-nofork")
            .arg("-ttymode")
            .arg("-f")
            .arg(format!("{}", self.format_file.path().to_str().unwrap()))
            .arg("-subfile")
            .arg(format!("{}", self.sub_file.path().to_str().unwrap()))
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
        self.kill();
        //self.format_file.close();
        //self.sub_file.close();
    }
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
