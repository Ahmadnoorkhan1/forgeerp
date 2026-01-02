# Placeholder Dockerfile for a migrations job/container.

FROM debian:bookworm-slim
WORKDIR /app
CMD ["sh", "-lc", "echo 'migrations placeholder'"]


