extern crate futures;
extern crate reqwest;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio;
extern crate walkdir;

use clap::{App, Arg};
use futures::{future::ok, Future, Stream};

// use rayon::prelude::*;
use reqwest::r#async::{Client, Response};
use walkdir::{DirEntry, WalkDir};

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct FilmInfo {
    Title: String,
    Year: String,
    Runtime: String,
    Genre: String,
    Plot: String,
    Poster: String,
    imdbRating: String,
}

impl std::fmt::Display for FilmInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} ({}) -- {}\n{}\n{}",
            self.Title, self.Year, self.Runtime, self.Genre, self.Plot
        )
    }
}

#[allow(dead_code)]
#[derive(Debug)]
struct Film {
    file: String,
    info: FilmInfo,
}

fn is_valid_entry(entry: DirEntry) -> Option<String> {
    if let Some(base) = entry.file_name().to_str() {
        if !base.starts_with('.') && base.ends_with(".mkv") && !base.starts_with("Trailer_") {
            return Some(base.to_string());
        }
    }
    None
}

fn get_films(location: String) -> Vec<FilmInfo> {
    let base_url = "https://www.omdbapi.com/?apikey=1d4b6bb0&t=";
    let client = Client::new();
    let (s, r) = crossbeam_channel::unbounded();
    let json = |mut res: Response| res.json::<FilmInfo>();
    let get_resp = move |title: String| {
        client
            .get(&(base_url.to_owned() + &title))
            .send()
            .and_then(json)
    };

    std::thread::spawn(move || {
        WalkDir::new(location)
            .follow_links(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter_map(is_valid_entry)
            .for_each(|f| {
                if let Some(base) = f.split('.').next() {
                    if let Some(title) = base.split(" (").next() {
                        println!("FOUND POSSIBLE MOVIE: {}", title);
                        if let Err(e) = s.send(title.to_owned()) {
                            println!("{}", e);
                        }
                    }
                }
            });
        drop(s);
    });

    let (fs, fr) = crossbeam_channel::unbounded();
    tokio::run(
        futures::stream::iter_ok(r.into_iter())
            .map(get_resp)
            .buffer_unordered(4000)
            .for_each(move |f| {
                fs.send(f).unwrap();
                ok(())
            })
            .map_err(|e| println!("Error: {}", e)),
    );
    fr.iter().collect()
}

fn main() -> Result<(), ()> {
    use std::path::PathBuf;
    let moviedir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Videos")
        .join("Movies");

    let matches = App::new("Pixie")
        .version("0.1.0")
        .author("Theodore Pauw <theopaon@gmail.com>")
        .about("Pixiedust Films!")
        .arg(
            Arg::with_name("location")
                .short("l")
                .long("location")
                .takes_value(true)
                .default_value(moviedir.to_str().unwrap_or("./Videos/Movies"))
                .multiple(true)
                .help("Location of movie files"),
        )
        .arg(
            Arg::with_name("update")
                .short("u")
                .long("update")
                .help("Update database"),
        )
        .arg(
            Arg::with_name("search")
                .short("s")
                .long("search")
                .takes_value(true)
                .multiple(true)
                .help("Search for Movie Title"),
        )
        .get_matches();

    let location = matches.value_of("location").expect("Invalid location. Please double check and wrap in quotes.");

    if matches.is_present("search") {
        println!("Location: {}", location);
    }

    if matches.is_present("update") {
        let films = get_films(location.to_string());

        println!();
        let mut i = 1;
        films.iter().for_each(|f| {
            println!("{}. {}", i, f);
            i += 1;
        });
        println!();
        let mut input = String::new();
        match std::io::stdin().read_line(&mut input) {
            Ok(_) => match input.trim().parse::<usize>() {
                Ok(index) if index < films.len() => println!("\n{}", &films[index]),
                _ => println!("Ok! Nothing to be done."),
            },
            Err(e) => println!("{}", e),
        }
    }
    Ok(())
}
