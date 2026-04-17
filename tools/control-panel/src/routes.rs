use anyhow::{anyhow, Context, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{
    model::{
        ArcadeDriveRequest, GamepadUpdateRequest, MotorActionRequest, RunTestRequest,
        SensorOverrideRequest, SystemActionRequest,
    },
    transport::MockRobotTransport,
};

const INDEX_HTML: &str = include_str!("../static/index.html");

pub async fn serve(listener: TcpListener, transport: MockRobotTransport) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let transport = transport.clone();

        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, transport).await {
                eprintln!("control-panel connection error: {error:#}");
            }
        });
    }
}

async fn handle_connection(mut stream: TcpStream, transport: MockRobotTransport) -> Result<()> {
    let mut buffer = vec![0_u8; 64 * 1024];
    let bytes_read = stream.read(&mut buffer).await?;

    if bytes_read == 0 {
        return Ok(());
    }

    let raw_request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let (head, body) = split_request(&raw_request)?;
    let mut head_lines = head.lines();
    let request_line = head_lines
        .next()
        .ok_or_else(|| anyhow!("missing HTTP request line"))?;

    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP method"))?;
    let path = request_parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP path"))?;

    let response = route_request(method, path, body, transport)
        .await
        .unwrap_or_else(render_error_response);

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}

async fn route_request(
    method: &str,
    path: &str,
    body: &str,
    transport: MockRobotTransport,
) -> Result<String> {
    match (method, path) {
        ("GET", "/") => Ok(html_response(INDEX_HTML)),
        ("GET", "/index.html") => Ok(html_response(INDEX_HTML)),
        ("GET", "/favicon.ico") => Ok(text_response("", "image/x-icon", "")),
        ("GET", "/api/state") => Ok(json_response(&transport.snapshot().await)?),
        ("POST", "/api/heartbeat") => Ok(json_response(&transport.heartbeat().await)?),
        ("POST", "/api/tests/run") => {
            let request: RunTestRequest =
                serde_json::from_str(body).context("failed to parse run test request body")?;
            Ok(json_response(&transport.run_test(request).await)?)
        }
        ("POST", "/api/motors/action") => {
            let request: MotorActionRequest =
                serde_json::from_str(body).context("failed to parse motor action request body")?;
            Ok(json_response(
                &transport.apply_motion_action(request).await,
            )?)
        }
        ("POST", "/api/motors/arcade") => {
            let request: ArcadeDriveRequest =
                serde_json::from_str(body).context("failed to parse arcade drive request body")?;
            Ok(json_response(&transport.apply_arcade_drive(request).await)?)
        }
        ("POST", "/api/sensors") => {
            let request: SensorOverrideRequest = serde_json::from_str(body)
                .context("failed to parse sensor override request body")?;
            Ok(json_response(&transport.update_sensors(request).await)?)
        }
        ("POST", "/api/gamepad") => {
            let request: GamepadUpdateRequest = serde_json::from_str(body)
                .context("failed to parse gamepad update request body")?;
            Ok(json_response(&transport.update_gamepad(request).await)?)
        }
        ("POST", "/api/system") => {
            let request: SystemActionRequest =
                serde_json::from_str(body).context("failed to parse system action request body")?;
            Ok(json_response(
                &transport.apply_system_action(request).await,
            )?)
        }
        _ => Ok(text_response(
            "404 Not Found",
            "text/plain; charset=utf-8",
            "route not found",
        )),
    }
}

fn split_request(raw_request: &str) -> Result<(&str, &str)> {
    raw_request
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("invalid HTTP request: missing CRLF separator"))
}

fn json_response<T: serde::Serialize>(value: &T) -> Result<String> {
    let body = serde_json::to_string(value)?;
    Ok(text_response(
        "200 OK",
        "application/json; charset=utf-8",
        &body,
    ))
}

fn html_response(body: &str) -> String {
    text_response("200 OK", "text/html; charset=utf-8", body)
}

fn text_response(status: &str, content_type: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.as_bytes().len()
    )
}

fn render_error_response(error: anyhow::Error) -> String {
    let body = format!(
        "{{\"error\":\"{}\"}}",
        escape_json_string(&error.to_string())
    );
    text_response("400 Bad Request", "application/json; charset=utf-8", &body)
}

fn escape_json_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}
