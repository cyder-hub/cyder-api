Cyder API
===

Cyder API is a Rust-based API service designed for AI applications. It provides a robust and scalable backend for handling various AI-related tasks.

## Configuration

The configuration for Cyder API is managed via a `config.yaml` file. Below are the key configuration options:
- `host`: The IP address the API will bind to.
- `port`: The port number the API will listen on.
- `base_path`: The base path for all API endpoints.
- `secret_key`: A secret key used for various cryptographic operations.
- `password_salt`: A salt used for hashing passwords.
- `jwt_secret`: A secret key used for signing JSON Web Tokens (JWT).
- `db_url`: The URL for the database connection. This can be a local SQLite database or a remote PostgreSQL or MySQL database.
- `proxy`: Configuration for a proxy server, if needed.

## Docker

Cyder API can be easily deployed using Docker. The provided `Dockerfile` builds the application and sets up the necessary environment.

### Building the Docker Image
To build the Docker image, navigate to the root directory of the project and run the following command:
docker build -t cyder-api:latest .

tips: chenluo/cyder-api-base is a base image that includes Rust and other dependencies required for building the Cyder API. It is based on Alpine Linux 3.20.