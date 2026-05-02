use std::io::{self, Write};

pub fn flush_stdout() -> io::Result<()> {
    std::io::stdout().flush()
}

pub fn write_stdout(content: &str) -> io::Result<()> {
    print!("{}", content);
    flush_stdout()
}