//! Profiler for tracing the execution of the RPC server.

use once_cell::sync::Lazy;
use serde_json::{json, Map, Value};
use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    sync::Mutex,
    thread::current,
    time::Instant,
};
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

/// Global profiler instance initialized lazily
static PROFILER: Lazy<Mutex<Profiler>> = Lazy::new(|| {
    Mutex::new(Profiler {
        genesis: Instant::now(),
        raw_events: HashMap::new(),
        multi_recorder: HashMap::new(),
    })
});

#[derive(Debug)]
struct Profiler {
    genesis: Instant,
    raw_events: HashMap<
        String, // thread id
        HashMap<
            String, // task name
            Vec<(
                u128,               // start time
                Option<u128>,       // end time (optional)
                Map<String, Value>, // other description
            )>,
        >,
    >,
    multi_recorder: HashMap<
        String,       // task name
        (u128, bool), // occurrence count & is ended
    >,
}

impl Profiler {
    /// Returns a reference to the global profiler instance
    fn global() -> &'static Mutex<Profiler> {
        &PROFILER
    }

    /// Gets the current thread's name as a string
    fn get_current_thread_name() -> String {
        let thread_id = current().id();
        format!("{:?}", thread_id)
    }

    /// Inserts a new thread into the profiler if it doesn't exist
    /// Returns true if the thread was newly inserted, false if it already existed
    fn insert_thread(&mut self, thread: &str) -> bool {
        let non_exist = !self.raw_events.contains_key(thread);
        if non_exist {
            self.raw_events.insert(thread.to_string(), HashMap::new());
        }
        non_exist
    }

    /// Inserts a new task for a specific thread if it doesn't exist
    /// Returns true if the task was newly inserted, false if it already existed
    fn insert_thread_task(&mut self, task: &str, thread: &str) -> bool {
        self.insert_thread(thread);
        let raw_events = self.raw_events.get_mut(thread).unwrap();
        let non_exist = !raw_events.contains_key(task);
        if non_exist {
            raw_events.insert(task.to_string(), vec![]);
        }
        non_exist
    }

    /// Inserts a new task for the current thread if it doesn't exist
    /// Returns true if the task was newly inserted, false if it already existed
    fn insert_current_thread_task(&mut self, task: &str) -> bool {
        self.insert_thread_task(task, &Profiler::get_current_thread_name())
    }

    /// Gets a reference to the events vector for a specific task and thread
    /// Panics if either the thread or task don't exist
    fn must_get(&self, task: &str, thread: &str) -> &Vec<(u128, Option<u128>, Map<String, Value>)> {
        self.raw_events.get(thread).unwrap().get(task).unwrap()
    }

    /// Gets a reference to the events vector for a specific task in the current thread
    /// Panics if either the thread or task don't exist
    fn must_get_current(&self, task: &str) -> &Vec<(u128, Option<u128>, Map<String, Value>)> {
        self.raw_events
            .get(&Profiler::get_current_thread_name())
            .unwrap()
            .get(task)
            .unwrap()
    }

    /// Gets a mutable reference to the events vector for a specific task and thread
    /// Panics if either the thread or task don't exist
    fn must_get_mut(
        &mut self,
        task: &str,
        thread: &str,
    ) -> &mut Vec<(u128, Option<u128>, Map<String, Value>)> {
        self.raw_events
            .get_mut(thread)
            .unwrap()
            .get_mut(task)
            .unwrap()
    }

    /// Gets a mutable reference to the events vector for a specific task in the current thread
    /// Panics if either the thread or task don't exist
    fn must_get_mut_current(
        &mut self,
        task: &str,
    ) -> &mut Vec<(u128, Option<u128>, Map<String, Value>)> {
        self.raw_events
            .get_mut(&Profiler::get_current_thread_name())
            .unwrap()
            .get_mut(task)
            .unwrap()
    }

    /// Clears all profiling data from the profiler
    fn clear(&mut self) {
        self.raw_events.clear();
    }
}

/// Gets the genesis time when the profiler was initialized
pub fn get_genesis() -> Instant {
    let profiler = Profiler::global().lock().unwrap();
    profiler.genesis
}

/// Starts timing a new task in the current thread
/// Panics if the last event for this task is not ended
pub fn start(task: &str) {
    let mut profiler = Profiler::global().lock().unwrap();
    let genesis = profiler.genesis;
    match profiler.insert_current_thread_task(task) {
        false => assert!(
            profiler.must_get_current(task).last().unwrap().1.is_some(),
            "the last event must be end"
        ),
        true => (),
    };
    profiler.must_get_mut_current(task).push((
        Instant::now().duration_since(genesis).as_nanos(),
        None,
        Map::new(),
    ));
}

/// Starts timing a new task that may be called multiple times with the same name
/// Only works in the main thread
pub fn start_multiple_times_in_main_thread(base_task: &str) {
    let mut profiler = Profiler::global().lock().unwrap();
    let genesis = profiler.genesis;
    let count = match profiler.multi_recorder.get_mut(base_task) {
        None => {
            profiler
                .multi_recorder
                .insert(base_task.to_string(), (1, false));
            0
        }
        Some((count, is_ended)) => {
            assert!(*is_ended, "the last event must be end");
            *count += 1;
            *is_ended = false;
            *count - 1
        }
    };
    let task = &format!("{}-[{}]", base_task, count);
    match profiler.insert_thread_task(task, "main") {
        false => assert!(
            profiler.must_get(task, "main").last().unwrap().1.is_some(),
            "the last event must be end"
        ),
        true => (),
    };
    profiler.must_get_mut(task, "main").push((
        Instant::now().duration_since(genesis).as_nanos(),
        None,
        Map::new(),
    ));
}

/// Ends timing for a task in the current thread
/// Panics if the last event for this task was not started
pub fn end(task: &str) {
    let mut profiler = Profiler::global().lock().unwrap();
    assert!(
        profiler.must_get_current(task).last().unwrap().1.is_none(),
        "the last event must be start"
    );
    profiler.must_get_mut_current(task).last_mut().unwrap().1 =
        Some(Instant::now().duration_since(profiler.genesis).as_nanos());
}

/// Ends timing for a task that was called multiple times
/// Only works in the main thread
pub fn end_multiple_times_in_main_thread(base_task: &str) {
    let mut profiler = Profiler::global().lock().unwrap();
    let (count, is_ended) = profiler.multi_recorder.get_mut(base_task).unwrap();
    assert!(!*is_ended, "the last event must not be end");
    *is_ended = true;
    let task = &format!("{}-[{}]", base_task, *count - 1);
    assert!(
        profiler.must_get(task, "main").last().unwrap().1.is_none(),
        "the last event must be start"
    );
    profiler.must_get_mut(task, "main").last_mut().unwrap().1 =
        Some(Instant::now().duration_since(profiler.genesis).as_nanos());
}

/// Adds a key-value note to the last event of a task
/// Panics if the last event was not started
pub fn note(task: &str, key: &str, value: Value) {
    let mut profiler = Profiler::global().lock().unwrap();
    assert!(
        profiler.must_get_current(task).last().unwrap().1.is_none(),
        "the last event must be start"
    );
    profiler
        .must_get_mut_current(task)
        .last_mut()
        .unwrap()
        .2
        .insert(key.to_string(), value);
}

/// Adds a string key-value note to the last event of a task
pub fn note_str(task: &str, key: &str, value: &str) {
    note(task, key, Value::String(value.to_string()));
}

/// Adds multiple key-value notes to the last event of a task
pub fn notes(task: &str, description: &mut Map<String, Value>) {
    let mut profiler = Profiler::global().lock().unwrap();
    profiler
        .must_get_mut_current(task)
        .last_mut()
        .unwrap()
        .2
        .append(description);
}

/// Adds the current time as a value for a key in the last event of a task
pub fn note_time(task: &str, key: &str) {
    let mut profiler = Profiler::global().lock().unwrap();
    let genesis = profiler.genesis;
    assert!(
        profiler.must_get_current(task).last().unwrap().1.is_none(),
        "the last event must be start"
    );
    profiler
        .must_get_mut_current(task)
        .last_mut()
        .unwrap()
        .2
        .insert(
            key.to_string(),
            (Instant::now().duration_since(genesis).as_nanos() as u64).into(),
        );
}

/// Adds a string key-value note to the last event of a task that was called multiple times
/// Only works in the main thread
pub fn note_str_multiple_times_in_main_thread(base_task: &str, key: &str, value: &str) {
    let mut profiler = Profiler::global().lock().unwrap();
    let (count, is_ended) = profiler.multi_recorder.get_mut(base_task).unwrap();
    assert!(!*is_ended, "the last event must not be end");
    let task = &format!("{}-[{}]", base_task, *count - 1);
    assert!(
        profiler.must_get(task, "main").last().unwrap().1.is_none(),
        "the last event must be start"
    );
    profiler
        .must_get_mut(task, "main")
        .last_mut()
        .unwrap()
        .2
        .insert(key.to_string(), Value::String(value.to_string()));
}

/// Clears all profiling data
pub fn clear() {
    let mut profiler = Profiler::global().lock().unwrap();
    profiler.clear();
}

/// Dumps the profiler data as a JSON string
pub fn dump() -> String {
    let profiler = Profiler::global().lock().unwrap();
    let now = Instant::now().duration_since(profiler.genesis).as_nanos();

    let mut output_frontend: Map<String, Value> = Map::new();
    output_frontend.insert("title".into(), Value::Object(Map::new()));
    output_frontend.insert("details".into(), Value::Array(vec![]));

    for (_thread_name, thread_events) in &profiler.raw_events {
        let mut detail = vec![];
        for (name, raw_events) in thread_events {
            for event in raw_events {
                let (start, end_opt, description) = event;
                let duration = end_opt.unwrap_or(now) - start;

                match description.get("type") {
                    Some(Value::String(type_str)) => match type_str.as_str() {
                        "transaction" => detail.push(json!({
                            "type": "transaction",
                            "tx": name,
                            "runtime": duration,
                            "start": start,
                            "end": end_opt,
                            // "status": description.get("status").unwrap().as_str().unwrap_or("unknown"),
                            "status": match description.get("status") {
                                Some(value) => value.as_str().unwrap_or("unknown"),
                                None => "unknown",
                            },
                            "detail": description,
                        })),
                        "commit" => detail.push(json!({
                            "type": "commit",
                            "tx": match description.get("tx") {
                                Some(value) => value.as_str().unwrap_or("unknown"),
                                None => "unknown",
                            },
                            "runtime": duration,
                            "start": start,
                            "end": end_opt,
                            "detail": description,
                        })),
                        other_type => detail.push(json!({
                            "type": other_type,
                            "name": name,
                            "runtime": duration,
                            "start": start,
                            "end": end_opt,
                            "detail": description,
                        })),
                    },
                    _ => detail.push(json!({
                        "type": "other",
                        "name": name,
                        "runtime": duration,
                        "start": start,
                        "end": end_opt,
                        "detail": description,
                    })),
                }
            }
        }
        output_frontend
            .get_mut("details")
            .unwrap()
            .as_array_mut()
            .unwrap()
            .push(Value::Array(detail));
    }

    serde_json::to_string_pretty(&output_frontend).unwrap()
}

/// Dumps the profiler data to a JSON file at the specified path
pub fn dump_json(output_path: &str) {
    let result_json = dump();
    let mut file = File::create(output_path).unwrap();
    file.write_all(result_json.as_bytes()).unwrap();
}

/// Dumps the profiler data to a ZIP file containing a JSON file
pub fn dump_zip(output_name: &str) {
    let result_json = dump();
    let file = File::create(output_name.to_string() + ".zip").unwrap();
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let options = FileOptions::<()>::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(output_name.to_string() + ".json", options)
        .unwrap();
    zip.write_all(result_json.as_bytes()).unwrap();
    zip.finish().unwrap();
}

/// Prints the current state of the profiler for debugging
pub fn debug_print() {
    println!("Profiler: {:?}", PROFILER);
}
