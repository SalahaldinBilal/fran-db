mod conformance;
mod docker;
mod mysql;
mod postgres;
mod servers;
mod support;

use std::{collections::HashMap, sync::Arc, thread};

use libtest_mimic::{Arguments, Failed, Trial};

use servers::{SPECS, Server, Spec};
use support::Case;

type Started = HashMap<&'static str, Result<Arc<Server>, String>>;

fn trial_name(spec: &Spec, case: &Case) -> String {
    format!("{}::{}", spec.id, case.name)
}

fn selected(args: &Arguments, name: &str) -> bool {
    let matches = |pattern: &str| if args.exact { name == pattern } else { name.contains(pattern) };
    args.filter.as_deref().is_none_or(matches) && !args.skip.iter().any(|skip| matches(skip))
}

/// Starts only the servers that some selected test needs, all at once.
fn start_servers(needed: Vec<&'static Spec>) -> (Started, Option<docker::DockerHost>) {
    let mut started = Started::new();
    let mut docker_host = None;

    if needed.iter().any(|spec| spec.uses_container()) {
        match docker::prepare() {
            Ok(host) => docker_host = Some(host),
            Err(error) => {
                for spec in needed.iter().filter(|spec| spec.uses_container()) {
                    started.insert(spec.id, Err(error.clone()));
                }
            }
        }
    }

    let pending: Vec<&'static Spec> = needed.into_iter().filter(|spec| !started.contains_key(spec.id)).collect();
    thread::scope(|scope| {
        let handles: Vec<_> = pending
            .iter()
            .map(|&spec| (spec.id, scope.spawn(move || servers::start(spec))))
            .collect();
        for (id, handle) in handles {
            let result = handle
                .join()
                .unwrap_or_else(|_| Err(format!("{id} panicked while starting")));
            started.insert(id, result.map(Arc::new));
        }
    });

    (started, docker_host)
}

fn main() {
    let args = Arguments::from_args();
    let cases: Vec<Case> = [conformance::all(), mysql::all(), postgres::all()].concat();

    let planned: Vec<(&'static Spec, Case)> = SPECS
        .iter()
        .flat_map(|spec| cases.iter().filter(|case| case.applies_to(spec)).map(move |case| (spec, *case)))
        .collect();

    let (started, docker_host) = if args.list {
        (Started::new(), None)
    } else {
        let mut needed: Vec<&'static Spec> = Vec::new();
        for (spec, case) in &planned {
            if selected(&args, &trial_name(spec, case)) && !needed.iter().any(|known| known.id == spec.id) {
                needed.push(spec);
            }
        }
        start_servers(needed)
    };

    for (id, result) in &started {
        if let Err(error) = result {
            eprintln!("\n{id} isn't available, its tests will fail:\n  {error}\n");
        }
    }

    let trials: Vec<Trial> = planned
        .into_iter()
        .map(|(spec, case)| {
            let server = started.get(spec.id).cloned();
            Trial::test(trial_name(spec, &case), move || match server {
                Some(Ok(server)) => support::run_case(&server, case),
                Some(Err(_)) => Err(Failed::from(format!("{} isn't available, see the error above", spec.id))),
                None => Err(Failed::from(format!("{} wasn't started", spec.id))),
            })
        })
        .collect();

    let conclusion = libtest_mimic::run(&args, trials);
    drop(started);
    drop(docker_host);
    conclusion.exit();
}
