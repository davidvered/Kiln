use clap::{App, AppSettings, Arg, SubCommand};
use futures::prelude::Future;
use shiplift::{builder::PullOptions, Docker, builder::LogsOptions, tty::StreamType};
use tokio::prelude::*;

use std::boxed::Box;

fn main() {
    let matches = App::new("Kiln CLI")
        .setting(AppSettings::SubcommandRequired)
        .arg(Arg::with_name("use-local-image")
            .long("use-local-image")
            .help("Do not try and pull the latest version of a tool image. Useful for development and scanning without network access"))
        .subcommand(SubCommand::with_name("ruby")
            .about("perform security testing of Ruby based projects")
            .setting(AppSettings::SubcommandRequired)
            .subcommand(SubCommand::with_name("dependencies")
                .about("Use Bundler-audit to find known vulnerabilities in project dependencies")
            )
        ).get_matches();

    let use_local_image = matches.is_present("use-local-image");
    let test_tool_image_name = "kiln/bundler-audit:latest"; 
    let test_tool_name = String::from("bundler-audit-kiln-container"); 
    match matches.subcommand() {
        ("ruby", Some(sub_m)) => {
            match sub_m.subcommand_name() {
                Some("dependencies") => {
                    let prep_fut = prepare_tool_image(test_tool_image_name.to_owned(), use_local_image);
                    tokio::run(prep_fut);
                        
                    let path = std::env::current_dir().unwrap().to_str().unwrap().to_string() + ":" + "/code";
                    let mut path_vec = Vec::new();
                    path_vec.push(path.as_ref());
                    
                    let docker = Docker::new();
                    let tool_container_future = docker
                    .containers()
                    .create(
                    &shiplift::ContainerOptions::builder(test_tool_image_name)
                    .name(&test_tool_name)
                    .attach_stdout(true)
                    .attach_stderr(true)
                    .auto_remove(true)
                    .volumes(path_vec)
                    .build(),)
                    .map_err(|e| eprintln!("Error: {}", e))
                    .and_then(|container| { 
                        let docker = Docker::new();
                        docker
                        .containers()
                        .get(&container.id)
                        .start()
                        .map_err(|e| eprintln!("Error: {}", e))
                    })
                    .and_then(move |_|{
                       let docker = Docker::new();
                       let log_future = docker
                      .containers()
                      .get(&test_tool_name)
                      .logs(&LogsOptions::builder().stdout(true).stderr(true).follow(true).build())
                      .for_each(|chunk| {
                          match chunk.stream_type {
                              StreamType::StdOut => println!("{}", chunk.as_string().unwrap()),
                              StreamType::StdErr => eprintln!("{}", chunk.as_string().unwrap()),
                              StreamType::StdIn => (),
                          }
                          Ok(())
                      })
                      .map_err(|e| eprintln!("Error: {}", e));
                      tokio::spawn(log_future);
                      Ok(())
                    });
                    tokio::run(tool_container_future); 
                },
                _ => unreachable!()
            }
        },
        _ => unreachable!()
    };
}

fn prepare_tool_image<T>(tool_image_name: T, use_local_image: bool) -> Box<dyn Future<Item=(), Error=()> + Send + 'static> 
    where T: AsRef<str> + std::fmt::Display + Send + 'static {
    let docker = Docker::new();
    if use_local_image {
        return Box::new(
            docker.images()
                .get(tool_image_name.as_ref())
                .inspect()
                .then(move |res| {
                    match res {
                        Ok(_) => futures::future::ok(()),
                        Err(err) => {
                            match &err {
                                shiplift::errors::Error::Fault{code, message: _} if *code == 404 => eprintln!("Could not find {} locally. Quitting!", tool_image_name),
                                _  => eprintln!("{}", err)
                            };
                            futures::future::err(())
                        }
                    }
                })
            );
    } else {
        let pull_options = PullOptions::builder()
            .image(tool_image_name.as_ref())
            .build();

        return Box::new(
            docker.images()
                .pull(&pull_options)
                .inspect(|item| println!("{}", item["status"].as_str().unwrap()))
                .collect()
                .then(move |res| {
                    match res {
                        Ok(_) => {
                            futures::future::ok(())
                        },
                        Err(err) => {
                            match &err {
                                shiplift::errors::Error::Fault{code, message: _} if *code == 404 => eprintln!("Could not find {} on Docker Hub. Quitting!", tool_image_name),
                                _  => eprintln!("{}", err)
                            };
                            futures::future::err(())
                        }
                    }
                })
            );
    }
}


