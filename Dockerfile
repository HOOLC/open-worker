# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS rust-build
WORKDIR /src
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY crates ./crates
RUN cargo build --release -p zork -p zork-gateway -p zork-agent -p zork-call

FROM node:22-bookworm AS node-build
RUN npm install -g vite-plus@0.1.20
WORKDIR /app
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/admin-ui/package.json ./apps/admin-ui/package.json
COPY packages/zork/package.json ./packages/zork/package.json
RUN vp install --frozen-lockfile --filter @zork/admin-ui
COPY tsconfig.json vite.config.ts ./
COPY apps ./apps
COPY packages ./packages
COPY scripts/build ./scripts/build
RUN vp run --filter @zork/admin-ui build

FROM node:22-bookworm-slim AS node
WORKDIR /app
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl git gh python3 ripgrep \
  && rm -rf /var/lib/apt/lists/*
COPY --from=rust-build /src/target/release/zork /usr/local/bin/zork
COPY --from=rust-build /src/target/release/zork-gateway /usr/local/bin/zork-gateway
COPY --from=rust-build /src/target/release/zork-agent /usr/local/bin/zork-agent
COPY --from=rust-build /src/target/release/zork-call /usr/local/bin/zork-call
COPY --from=rust-build /src/target/release/zork-gh /usr/local/bin/zork-gh
COPY --from=node-build /app/apps/admin-ui/dist /ui
EXPOSE 18790 3000 3001
CMD ["zork", "start", "--data", "/data", "--listen", "0.0.0.0"]
