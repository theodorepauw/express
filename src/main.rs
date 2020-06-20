extern crate futures;
extern crate open;
extern crate rayon;
extern crate reqwest;
extern crate rusqlite;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate tokio;
extern crate walkdir;

use clap::{App, Arg, SubCommand};
use futures::{future::ok, Future, Stream};

use reqwest::r#async::{Client, Response};
#[allow(unused_imports)]
use rusqlite::{Connection, NO_PARAMS};
use std::path::PathBuf;
use walkdir::{DirEntry, WalkDir};

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct FilmInfo {
    #[serde(default)]
    Title: String,
    #[serde(default)]
    Year: String,
    #[serde(default)]
    Runtime: String,
    #[serde(default)]
    Genre: String,
    #[serde(default)]
    Plot: String,
    #[serde(default)]
    Poster: String,
    #[serde(default)]
    imdbRating: String,
    #[serde(default)]
    imdbID: String,
    #[serde(default)]
    Response: String,
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

#[derive(Debug)]
struct Film {
    path: String,
    info: FilmInfo,
}

fn is_valid_entry(entry: DirEntry) -> Option<(String, String)> {
    if let Some(base) = entry.file_name().to_str() {
        if !base.starts_with('.') && base.ends_with(".mkv") && !base.ends_with("-trailer.mkv") {
            if let Some(path) = entry.path().to_str() {
                return Some((base.to_string(), path.to_string()));
            }
        }
    }
    None
}

fn get_films(location: String, output: crossbeam_channel::Sender<Film>) {
    // obviously, I should actually use a secrets file.
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
            .for_each(move |(base, path)| {
                if let Some(part) = &base.split('.').next() {
                    if let Some(title) = &part.split(" (").next() {
                        println!("FOUND POSSIBLE MOVIE: {}", title);
                        if let Err(e) = s.send((title.to_string(), path)) {
                            eprintln!("{}", e);
                        }
                    }
                }
            });
    });

    tokio::run(
        futures::stream::iter_ok(r.into_iter())
            .map(move |(title, path)| (get_resp(title), ok(()).map(|_| path)))
            .buffer_unordered(400)
            .for_each(move |(i, p)| {
                if &i.Response[0..1] == "T" {
                    if let Err(e) = output.send(Film { info: i, path: p }) {
                        eprintln!("{}", e);
                    }
                } else {
                    eprintln!(
                        "No movie found for `{}`! Please check that the file is named correctly.",
                        p
                    );
                }
                ok(())
            })
            .map_err(|e| eprintln!("Error: {}", e)),
    );
}

fn save_in_database(input: &crossbeam_channel::Receiver<Film>) -> Result<(), rusqlite::Error> {
    let mut conn = Connection::open("orbit.sqlite")?;

    conn.execute_batch("
    BEGIN;
    CREATE TABLE IF NOT EXISTS MOVIES (id integer not null primary key, path text not null unique, title text, year int, plot text, rating int, imdbID text, poster_url text);
    CREATE TABLE IF NOT EXISTS GENRES (id integer not null primary key, genre text not null unique);
    CREATE TABLE IF NOT EXISTS MOVIEGENRES (genre_id integer, movie_id integer, FOREIGN KEY(movie_id) REFERENCES MOVIES (id) ON DELETE CASCADE, FOREIGN KEY(genre_id) REFERENCES GENRES (id));
    COMMIT;
    ",)?;

    println!();
    let (s, r) = crossbeam_channel::unbounded();
    std::thread::spawn(move || {
        let mut i = 1;
        r.iter().for_each(|f: Film| { println!("{}. {}", i, f.info); i += 1; });
    });

    let tx = conn.transaction()?;
    input.iter().for_each(|f| {
        if let Ok(mut stmt) = tx.prepare_cached("INSERT OR IGNORE INTO MOVIES (path, title, year, plot, rating, imdbID, poster_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)") {
            if let Err(e) = stmt.execute(&[&f.path, &f.info.Title, &f.info.Year, &f.info.Plot, &f.info.imdbRating, &f.info.imdbID, &f.info.Poster]) {
                eprintln!("{}", e);
            } else {
                if let Err(e) = s.send(f) {
                    eprintln!("{}", e);
                }
            }
        }   
    });
    drop(s);
    tx.commit()?;

    println!("\nDatabase `orbit.sqlite` created.");
    Ok(())
}

fn main() -> Result<(), ()> {
    let moviedir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Videos")
        .join("Movies");

    let matches = App::new("BRU...")
        .version("0.1.0")
        .author("Theodore Pauw <theopaon@gmail.com>")
        .about("BRU Films!")
        .subcommand(
            SubCommand::with_name("update")
                .arg(
                    Arg::with_name("location")
                        .short("l")
                        .long("location")
                        .takes_value(true)
                        .default_value(moviedir.to_str().unwrap_or("./Videos/Movies"))
                        .multiple(true)
                        .help("Location of Movie Library"),
                )
                .help("Update database"),
        )
        .subcommand(
            SubCommand::with_name("search")
                .arg(
                    Arg::with_name("titles")
                        .required(true)
                        .multiple(true)
                        .help("Movies to search for"),
                )
                .help("Lookup movie details"),
        )
        .arg(
            Arg::with_name("database")
                .short("db")
                .long("database")
                .default_value("val: &'a str"),
        )
        .get_matches();

    let cli = true;

    if let Some(update_matches) = matches.subcommand_matches("update") {
        let location = update_matches
            .value_of("location")
            .expect("Invalid location. Please double check and wrap in quotes.");

        let (tx, rx) = crossbeam_channel::unbounded();
        get_films(location.to_string(), tx);
        if cli {
            if save_in_database(&rx).is_err() {
                eprintln!("Couldn't save anything in the database!");
            };
            println!();
        }
    // let mut i = 1;
    // let mut films = Vec::new();
    // rx.iter().for_each(|f| {
    //     println!("{}. {}", i, &f.info);
    //     films.push(f);
    //     i += 1;
    // });

    // println!("\nType your selection!\n");
    // let mut input = String::new();
    // match std::io::stdin().read_line(&mut input) {
    //     Ok(_) => match input.trim().parse::<usize>() {
    //         Ok(index) if index <= films.len() => {
    //             let i = index - 1;
    //             println!("\n{}\n\n{}", &films[i].info, &films[i].path);
    //             if let Err(e) = open::that(&films[i].path) {
    //                 eprintln!("{}", e);
    //             }
    //         }
    //         _ => println!("Ok! Nothing to be done."),
    //     },
    //     Err(e) => eprintln!("{}", e),
    // }
    } else if let Some(search_matches) = matches.subcommand_matches("search") {
        search_matches
            .values_of("titles")
            .expect("No movies specified for search...")
            .for_each(move |t| {
                dbg!(t);
            });
    }

    Ok(())
}
