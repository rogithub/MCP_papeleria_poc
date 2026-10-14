# Imagen de la prueba de vida. CI la publica en amd64 (el cluster es x86).
FROM docker.io/library/rust:1.98.0-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM docker.io/library/debian:bookworm-slim
COPY --from=build /src/target/release/mcp-papeleria-poc /usr/local/bin/mcp-papeleria-poc
# Usuario sin privilegios, numérico para que Kubernetes pueda exigir runAsNonRoot.
USER 10001:10001
ENV PORT=8080
EXPOSE 8080
CMD ["mcp-papeleria-poc"]
