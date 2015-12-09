use chrono::UTC;
use chrono::duration::Duration;
use git::GitRepo;
use notifier;
use repo_config::{RepoConfig, DeployMethod};
use server_config::Environment;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use task_manager::Runnable;
use users;
use uuid::Uuid;

pub struct DeployTask {
    pub repo: GitRepo,
    pub id: Uuid,
    pub env: Environment,
    pub logdir: String,
    pub host: String,
    pub secret: String,
}
impl Runnable for DeployTask {

    // TODO: this is a god damn mess and seriously needs to be refactored,
    // especially all of the logging.
    #[allow(unused_must_use)]
    fn run(&mut self) {
        let task_id = self.id.to_string();

        // Insert the checkout path for the current checkout to the environment
        self.env.insert("hookshot_checkout_path".to_owned(), self.repo.local_path.clone());

        // Insert git data into the environment
        // TODO: figure out if env type can get away without having to own its
        // keys and values
        self.env.insert("git_ref".to_owned(), self.repo.refstring.clone());
        self.env.insert("git_ref_type".to_owned(), self.repo.reftype.to_string());
        self.env.insert("git_commit_sha".to_owned(), self.repo.sha.clone());
        self.env.insert("git_repo_name".to_owned(), self.repo.name.clone());
        self.env.insert("git_repo_owner".to_owned(), self.repo.owner.clone());

        // Truncate the logfile and write "task running..."
        let logfile_path = Path::new(&self.logdir).join(format!("{}.log", task_id));
        let mut logfile = match File::create(&logfile_path) {
            Ok(logfile) => logfile,
            Err(_) => return println!("[{}]: could not open logfile for writing", &task_id),
        };
        logfile.write_all(b"\ntask running...\n");

        // Log the current user
        logfile.write_all(format!("system user: {}\n\n", users::get_current_username().unwrap_or("<none>".to_owned())).as_bytes());

        // Log the hookshot environment variables
        logfile.write_all(format!("hookshot environment:\n---------------------\n{}\n", format_environment(&self.env)).as_bytes());

        // Log the system environment variables
        logfile.write_all(format!("system environment:\n-------------------\n{}\n", format_os_environment()).as_bytes());

        // Log what time the task started.
        let time_task_started = UTC::now();
        logfile.write_all(format!("started: {}\n", time_task_started).as_bytes());

        if let Err(git_error) = self.repo.get_latest() {
            let stderr = String::from_utf8(git_error.output.unwrap().stderr).unwrap();
            let err = format!("{}: {}", git_error.desc, stderr);
            logfile.write_all(format!("{}", err).as_bytes());
            return println!("[{}]: {}", task_id, err);
        }

        let project_root = Path::new(&self.repo.local_path);
        let config = match RepoConfig::load(&project_root) {
            Err(e) => {
                let err = format!("could not load config for repo {}: {} (branch: {})",
                                  self.repo.remote_path,
                                  e.description(),
                                  e.related_branch().unwrap_or("None"));
                logfile.write_all(format!("{}", err).as_bytes());
                return println!("[{}]: {}", &task_id, err);
            }
            Ok(config) => config,
        };

        notifier::started(&self, &config);

        let ref_config = match config.lookup(self.repo.reftype, &self.repo.refstring) {
            None => {
                let err = format!("No config for ref '{}'", &self.repo.refstring);
                logfile.write_all(format!("{}", err).as_bytes());
                return println!("[{}]: {}", &task_id, err);
            }
            Some(config) => config,
        };

        // TODO: refactor this, use a trait or something.
        let output_result = {
            match ref_config.method {
                DeployMethod::Ansible => match ref_config.ansible_task() {
                    None => {
                        let err = format!("No task for ref '{}'", &self.repo.refstring);
                        logfile.write_all(format!("{}", err).as_bytes());
                        return println!("[{}]: {}", &task_id, err);
                    }
                    Some(task) => {
                        println!("[{}]: {:?}", &task_id, task);
                        println!("[{}]: with environment {:?}", &task_id, &self.env);
                        task.run(&self.env)
                    }
                },
                DeployMethod::Makefile => match ref_config.make_task() {
                    None => {
                        let err = format!("No task for ref '{}'", &self.repo.refstring);
                        logfile.write_all(format!("{}", err).as_bytes());
                        return println!("[{}]: {}", &task_id, err);
                    }
                    Some(task) => {
                        println!("[{}]: {:?}", self.id, task);
                        println!("[{}]: with environment {:?}", self.id, &self.env);
                        task.run(&self.env)
                    }
                },
            }
        };

        let output = match output_result {
            Ok(output) => output,
            Err(e) => {
                let err = format!("task failed: {} ({})",
                                  e.desc,
                                  e.detail.unwrap_or(String::from("")));
                logfile.write_all(format!("{}", err).as_bytes());
                return println!("[{}]: {}", &task_id, err);
            }
        };

        let exit_code = match output.status.code() {
            None => String::from("killed"),
            Some(code) => format!("{}", code),
        };

        let exit_status = match output.status.success() {
            true => {
                notifier::success(&self, &config);
                "successful"
            }
            false => {
                notifier::failed(&self, &config);
                "failed"
            }
        };
        println!("[{}]: run {}", self.id, exit_status);

        // Log what time the task ended and how long it took
        let time_task_ended = UTC::now();
        let duration = time_task_ended - time_task_started;
        logfile.write_all(format!("task finished: {}\n", time_task_ended).as_bytes());
        logfile.write_all(format!("duration: {}...\n\n", format_duration(duration)).as_bytes());

        // Log the exit code and the standard streams
        logfile.write_all(format!("exit code: {}.\n", exit_code).as_bytes());
        logfile.write_all(b"\n==stdout==\n");
        logfile.write_all(&output.stdout);
        logfile.write_all(b"\n==stderr==\n");
        logfile.write_all(&output.stderr);
    }
}


fn format_duration(duration: Duration) -> String {
    let mut minutes = 0i64;
    let mut seconds = duration.num_seconds();
    if seconds >= 60 {
        minutes = seconds / 60;
        seconds = seconds % 60;
    }
    match (minutes, seconds) {
        (0, 1) => format!("{} second", seconds),
        (0, _) => format!("{} seconds", seconds),
        (1, 0) => format!("{} minute", minutes),
        (1, 1) => format!("{} minute, {} second", minutes, seconds),
        (1, _) => format!("{} minute, {} seconds", minutes, seconds),
        (_, _) => format!("{} minutes, {} seconds", minutes, seconds),
    }
}

fn format_environment(env: &Environment) -> String {
    let mut env_string = String::new();
    for (k, v) in env.iter() {
        env_string.push_str(&format!("{}: {}\n", k, v))
    }
    env_string
}

fn format_os_environment() -> String {
    let mut env_string = String::new();
    for (k, v) in env::vars() {
        env_string.push_str(&format!("{}: {}\n", k, v))
    }
    env_string
}
