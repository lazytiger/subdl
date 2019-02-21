extern crate unrar;

use std::io::Error;
use std::path::Path;
use unrar::archive::OpenArchive;
use unrar::{archive::Entry, error::UnrarError, Archive};

#[derive(Debug)]
enum MyError {
    Archive(UnrarError<OpenArchive>),
    Entry(UnrarError<Vec<Entry>>),
    IO(Error),
}

impl From<Error> for MyError {
    fn from(err: Error) -> Self {
        MyError::IO(err)
    }
}

impl From<UnrarError<OpenArchive>> for MyError {
    fn from(err: UnrarError<OpenArchive>) -> MyError {
        MyError::Archive(err)
    }
}

impl From<UnrarError<Vec<Entry>>> for MyError {
    fn from(err: UnrarError<Vec<Entry>>) -> MyError {
        MyError::Entry(err)
    }
}

fn do_unrar(name: &str, path: &str) -> Result<Vec<String>, MyError> {
    let test = Archive::new(name.to_string());
    let target = std::env::temp_dir()
        .to_path_buf()
        .to_str()
        .unwrap()
        .to_string();
    let ret = test.extract_to(target.to_string())?.process()?;
    let mut files = Vec::new();
    for e in &ret {
        if e.is_directory() {
            continue;
        }
        let from = Path::new(&e.filename).to_path_buf();
        let to = Path::new(path).join(from.file_name().unwrap());
        std::fs::copy(&from, &to)?;
        files.push(from.file_name().unwrap().to_str().unwrap().to_string());
    }
    Ok(files)
}

fn main() {
    match do_unrar("/Users/hoping/test.rar", ".") {
        Err(err) => println!("error occurred {:?}", err),
        Ok(files) => {
            for e in &files {
                println!("file found {:?}", e);
            }
        }
    }
}
