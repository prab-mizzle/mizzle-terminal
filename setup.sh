
#!/bin/bash

set -e

# Detect package manager
if command -v apt >/dev/null; then
    PM="apt"
elif command -v dnf >/dev/null; then
    PM="dnf"
elif command -v yum >/dev/null; then
    PM="yum"
elif command -v pacman >/dev/null; then
    PM="pacman"
else
    echo "Unsupported package manager. Install Caddy manually."
    exit 1
fi

# Install Caddy
install_caddy() {
    echo "Installing Caddy..."
    case "$PM" in
        apt)
            sudo apt update -y && sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https -y
            curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg

            curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list

            sudo apt update && sudo apt install -y caddy
            ;;
        dnf|yum)
            sudo dnf install -y 'dnf-command(copr)'
            sudo dnf copr enable -y @caddy/caddy
            sudo dnf install -y caddy
            ;;
        pacman)
            sudo pacman -Syu --noconfirm caddy
            ;;
    esac
}

# Configure Caddy
# caddy server will run on port 8018 
configure_caddy() {
    echo "Setting up Caddy..."
    sudo tee /etc/caddy/Caddyfile >/dev/null <<EOF
:8018 {
        # Set this path to your site's directory.
        root * /usr/share/caddy

        # Enable the static file server.
        file_server

        # Another common task is to set up a reverse proxy:
        # reverse_proxy localhost:8080

        # Or serve a PHP site through php-fpm:
        # php_fastcgi localhost:9000
        @sessionPath {
            path_regexp session_id ^/([^/]+)
        }

        handle @sessionPath {
            rewrite * /
            reverse_proxy unix//tmp/{re.session_id.1}.sock
        }

        respond "Invalid route" 404
}
EOF

    sudo systemctl enable --now caddy
    echo "Caddy has been installed and configured! Access it at http://localhost:2301"
}

install_caddy
configure_caddy
