# Imagen de la prueba de vida. CI la publica en amd64 (el cluster es x86).
FROM docker.io/library/rust:1.98.0-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# Solo el relevo: el canal (`canal_papeleria`) corre en la computadora de la papelería.
RUN cargo build --release --locked --bin mcp-papeleria-poc

FROM docker.io/library/debian:bookworm-slim
# Certificados del sistema: el relevo baja las llaves de Cloudflare Access por HTTPS.
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/mcp-papeleria-poc /usr/local/bin/mcp-papeleria-poc
# Usuario sin privilegios, numérico para que Kubernetes pueda exigir runAsNonRoot.
USER 10001:10001
ENV PORT=8080
EXPOSE 8080
CMD ["mcp-papeleria-poc"]
