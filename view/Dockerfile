FROM debian:latest
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    git \
    libclang-dev
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y
RUN echo 'source $HOME/.cargo/env' >> $HOME/.bashrc
WORKDIR /view
COPY . .
RUN bash -c 'source ~/.bashrc; cargo build'
COPY docker-entrypoint.sh /docker-entrypoint.sh
CMD ["bash", "/docker-entrypoint.sh"]
