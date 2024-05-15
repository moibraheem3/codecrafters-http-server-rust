mod thread_pool;

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};

use self::thread_pool::ThreadPool;

#[derive(Debug, PartialEq, Clone)]
pub enum Method {
    GET,
}

#[derive(Debug, Clone)]
struct Handler {
    pub handler: fn(Request, Response),
    pub path: String,
    pub method: Method,
}

#[derive(Debug, Clone)]
pub struct Application {
    pub port: u16,
    handlers: Vec<Handler>,
}

impl Application {
    pub fn new(port: u16) -> Application {
        Application {
            port,
            handlers: vec![],
        }
    }

    pub fn listen(self, handler: fn(&Application)) {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", self.port)).unwrap();
        let pool = ThreadPool::new(4);
        handler(&self);
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let self_clone = self.clone();
                    pool.execute(move || self_clone.route(stream));
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                }
            }
        }
    }

    pub fn get(&mut self, path: &str, handler: fn(Request, Response)) {
        self.handlers.push(Handler {
            path: path.to_string(),
            handler,
            method: Method::GET,
        });
    }

    fn route(&self, mut stream: TcpStream) {
        let mut buf = [0; 1024];
        stream.read(&mut buf).unwrap();

        let (method, path) = Application::parse_request_line(&String::from_utf8_lossy(&buf[..]));

        let mut response = Response::new(200, stream);
        match method.as_str() {
            "GET" => {
                let request = Request::new(Method::GET, path.to_string());
                let gets = self
                    .handlers
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| h.method == Method::GET)
                    .map(|(_, h)| h)
                    .collect::<Vec<_>>();

                Application::router(gets, &path, request, response);
            }
            _ => {
                response.code(404).send();
            }
        }
    }

    fn parse_request_line(request: &Cow<str>) -> (String, String) {
        let request_line = request
            .lines()
            .next()
            .unwrap()
            .split(" ")
            .collect::<Vec<_>>();

        (request_line[0].to_string(), request_line[1].to_string())
    }

    fn router(handlers: Vec<&Handler>, path: &str, mut request: Request, mut response: Response) {
        for hd in handlers {
            let re = Application::parse_path(&hd.path);
            if re.is_match(&path) {
                let captures = re.captures(&path).unwrap();
                request.params = re
                    .capture_names()
                    .flatten()
                    .filter_map(|x| Some((x.to_string(), captures.name(x)?.as_str().to_string())))
                    .collect::<HashMap<String, String>>();
                (hd.handler)(request, response);
                return;
            }
        }
        response.code(404).send();
    }

    fn parse_path(path: &str) -> regex::Regex {
        let mut regex_str = "^".to_string();
        if path == "/" {
            if path.starts_with("/:") {
                regex_str.push_str(&format!(r"/(?P<{}>\w+)", path.get(2..).unwrap()))
            } else {
                regex_str.push_str("/")
            }
        } else {
            for str in path.split("/") {
                if !str.is_empty() {
                    if str.starts_with(":") {
                        regex_str.push_str(&format!(r"/(?P<{}>\w+)", str.get(1..).unwrap()))
                    } else {
                        regex_str.push_str(&format!("/{}", str))
                    }
                }
            }
        }
        regex_str.push_str("$");
        regex::Regex::new(&regex_str).unwrap()
    }
}

#[derive(Debug)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub params: HashMap<String, String>,
}

impl Request {
    pub fn new(method: Method, path: String) -> Request {
        Request {
            method,
            path,
            params: HashMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct Response {
    code: u16,
    stream: TcpStream,
}

impl Response {
    pub fn new(code: u16, stream: TcpStream) -> Response {
        Response { code, stream }
    }

    pub fn code(&mut self, code: u16) -> &mut Self {
        self.code = code;
        self
    }

    pub fn send_text(&mut self, data: &str) {
        self.write_res_code();
        self.stream
            .write("Content-Type: text/plain\r\n".as_bytes())
            .unwrap();
        self.stream
            .write(format!("Content-Length: {}\r\n", data.len()).as_bytes())
            .unwrap();
        self.stream.write("\r\n".as_bytes()).unwrap();
        self.stream.write(data.as_bytes()).unwrap();
        self.stream.shutdown(Shutdown::Both).unwrap();
    }

    pub fn send(&mut self) {
        self.write_res_code();
        self.stream.write("\r\n".as_bytes()).unwrap();
        self.stream.shutdown(Shutdown::Both).unwrap();
    }

    fn write_res_code(&mut self) {
        match self.code {
            200 => {
                self.stream.write("HTTP/1.1 200 OK\r\n".as_bytes()).unwrap();
            }
            404 => {
                self.stream
                    .write("HTTP/1.1 404 Not Found\r\n".as_bytes())
                    .unwrap();
            }
            500 | _ => {
                self.stream
                    .write("HTTP/1.1 500 Internal Server Error\r\n".as_bytes())
                    .unwrap();
            }
        }
    }
}
