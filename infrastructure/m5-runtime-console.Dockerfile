FROM node:22-alpine AS build
RUN corepack enable
WORKDIR /workspace
COPY --from=runtime-console . .
RUN pnpm install --frozen-lockfile
RUN VITE_RUNTIME_CONSOLE_MODE=api VITE_API_BASE_URL=/ pnpm build:local

FROM nginx:1.27-alpine
COPY --from=build /workspace/dist /usr/share/nginx/html
RUN printf '%s\n' \
    'server {' \
    '  listen 8080;' \
    '  root /usr/share/nginx/html;' \
    '  index index.html;' \
    '  location /health { default_type application/json; return 200 '\''{"status":"ready"}'\''; }' \
    '  location /admin/ { proxy_pass http://lenso-system-plane:8080; }' \
    '  location = / { try_files /index.html =404; }' \
    '  location / { try_files $uri $uri/ /index.html; }' \
    '}' > /etc/nginx/conf.d/default.conf
