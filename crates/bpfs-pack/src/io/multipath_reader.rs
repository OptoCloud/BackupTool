#![allow(dead_code)] // reserved for spanned/multi-volume archive sources

use std::fs::File;
use std::io::{self, Read};
use std::path::PathBuf;

pub struct MultiPathReader {
    paths: Vec<PathBuf>,
    cur: Option<File>,
    idx: usize,
}

impl MultiPathReader {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            cur: None,
            idx: 0,
        }
    }
}

impl Read for MultiPathReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.idx >= self.paths.len() {
                return Ok(0);
            }
            if self.cur.is_none() {
                self.cur = Some(File::open(&self.paths[self.idx])?);
            }
            let n = self.cur.as_mut().unwrap().read(buf)?;
            if n > 0 {
                return Ok(n);
            }
            self.cur = None;
            self.idx += 1;
        }
    }
}
