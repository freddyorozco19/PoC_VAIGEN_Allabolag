# Despliegue de Siffra (Rust) en el VPS

Resumen de cómo está desplegado `siffra.architechia.co` (VPS `177.7.46.87`, Ubuntu 22.04). Sin secretos:
las claves viven solo en el servidor.

## Arquitectura

```
Internet ──443──▶ nginx (HTTPS con certbot, noindex, HSTS, limit_req; el acceso lo controla la aplicación)
                   └─▶ 127.0.0.1:3011 ── pm2 "siffra" ── /root/siffra/siffra-rs/target/release/siffra-rs
```

- Un único binario (el CSS va dentro). Cuentas, sesiones y registro de actividad en SQLite (`/root/siffra/data/siffra.db`, carpeta 700, archivo 600). Las cachés (SCB, empresas, cuentas anuales) viven en memoria.
- Puerto interno **3011** (el 3010 ya lo usa otra app). Solo escucha en `127.0.0.1`.
- Configuración en `/root/siffra/siffra-rs/.env.local` (permisos 600): `BOLAGSVERKET_CLIENT_ID`,
  `BOLAGSVERKET_CLIENT_SECRET` y `BOLAGSVERKET_BASE_URL=https://gw.api.bolagsverket.se/vardefulla-datamangder/v1`
  (claves de PRODUCCIÓN; el token se deduce de la URL base). `PORT` lo fija pm2.
- Acceso: inicio de sesión de la propia aplicación (roles superadmin / admin / user). La clave básica de nginx (`/etc/nginx/.htpasswd-siffra`) ya no se usa; copia de la configuración anterior en `/root/siffra/nginx-siffra.bak-20261005`.
- pm2 fija `PORT=3011`, `SIFFRA_DB` y `SIFFRA_SECURE_COOKIES=1` (ver `deploy/ecosystem.config.cjs`).

## Primera vez

1. DNS (GoDaddy): registro `A`, nombre `siffra`, valor `177.7.46.87`.
2. Rust en el servidor: `curl https://sh.rustup.rs | sh -s -- -y --profile minimal`.
3. Subir el código (sin `target`) y compilar con tope de memoria (el VPS comparte máquina con otros servicios):
   ```bash
   tar --exclude=target --exclude=.git -czf - siffra-rs | ssh root@177.7.46.87 'mkdir -p /root/siffra && tar -xzf - -C /root/siffra'
   ssh root@177.7.46.87 'cd /root/siffra/siffra-rs && . /root/.cargo/env && \
     systemd-run --scope -p MemoryMax=1500M -p MemorySwapMax=0 nice -n 19 env CARGO_BUILD_JOBS=1 cargo build --release'
   ```
   (≈2 min con 1 hilo.)
4. Crear `/root/siffra/siffra-rs/.env.local` con las claves y `chmod 600`.
5. pm2: `pm2 start deploy/ecosystem.config.cjs && pm2 save` (pm2 está instalado vía nvm: `/root/.nvm/versions/node/v20.20.2/bin`).
6. nginx: copiar `deploy/nginx-siffra.conf` a `/etc/nginx/sites-available/siffra`, enlazar en `sites-enabled`,
   `nginx -t && systemctl reload nginx`.
7. HTTPS: `certbot --nginx -d siffra.architechia.co` (una vez que el DNS resuelva). Emite y renueva el certificado
   (válido hasta 2027-01-03; renovación automática con `certbot.timer`), **pero en este servidor el instalador de
   nginx falla** con `Unsupported RSA key length: 1024`: para sitios nuevos certbot genera un certificado temporal
   de 1024 bits y las librerías criptográficas actuales lo rechazan. Solución usada: escribir a mano el bloque
   `listen 443 ssl` (ya incluido en `deploy/nginx-siffra.conf`). Con el bloque HTTPS ya presente, `certbot renew`
   no necesita ese certificado temporal (`certbot renew --cert-name siffra.architechia.co --dry-run` lo confirma).

## Actualizar

```bash
tar --exclude=target --exclude=.git -czf - siffra-rs | ssh root@177.7.46.87 'tar -xzf - -C /root/siffra'   # no pisa .env.local ni target
ssh root@177.7.46.87 'cd /root/siffra/siffra-rs && . /root/.cargo/env && \
  systemd-run --scope -p MemoryMax=1500M -p MemorySwapMax=0 nice -n 19 env CARGO_BUILD_JOBS=1 cargo build --release && \
  /root/.nvm/versions/node/v20.20.2/bin/pm2 restart siffra'
```

## Operación

- Logs: `pm2 logs siffra`. Debe aparecer `Bolagsverket: conectado a gw.api.bolagsverket.se`.
- Reiniciar: `pm2 restart siffra`. Estado: `pm2 list`.
- Si cambian las claves: editar `.env.local` y `pm2 restart siffra`.
- Bolagsverket cierra las cuentas inactivas a los 6 meses y limita a 60 consultas/minuto en total.

## Pendiente antes de abrirlo al público

- Alojar las tipografías (hoy se cargan desde Google Fonts: envía la IP de cada visitante a Google; relevante para RGPD).
- Endurecer la CSP: hoy permite `unsafe-inline` porque la app usa scripts y estilos en línea (requiere nonces o hashes). HSTS ya está activo (1 año, sin subdominios).
- Revisar las condiciones de uso de la API de Bolagsverket y decidir si las 3 empresas ficticias de ejemplo se quedan.

## Cuentas y base de datos

Cuentas actuales (2026-10-05): superadmin `architechia` (conserva la contraseña que ya tenía), admin
`freddy.orozco@architechia.co` y user `testing@architechia.co` (estas dos con contraseña temporal que obliga a cambiarla
en el primer acceso). Se crean y gestionan desde la propia aplicación (`/users`) o por línea de comandos:

```bash
ssh root@177.7.46.87
cd /root/siffra/siffra-rs && export SIFFRA_DB=/root/siffra/data/siffra.db
./target/release/siffra-rs user-list
./target/release/siffra-rs user-reset architechia                     # contraseña temporal nueva (se pide cambiarla al entrar)
SIFFRA_NEW_PW='...' ./target/release/siffra-rs user-reset architechia --password-env SIFFRA_NEW_PW
./target/release/siffra-rs user-add --role admin --name "Nombre" --email persona@architechia.co
```

Si se olvida la contraseña del único superadmin, `user-reset` desde el servidor la recupera (no hace falta tocar la base de datos).

**Copia de seguridad** (el destino no debe existir; `VACUUM INTO` hace una copia consistente con el servidor en marcha):

```bash
rm -f /root/siffra/data/backup.db && SIFFRA_DB=/root/siffra/data/siffra.db /root/siffra/siffra-rs/target/release/siffra-rs backup /root/siffra/data/backup.db
```

Convendría programarla (cron diario) y copiarla fuera del VPS: la base contiene los hashes de las contraseñas y el registro de actividad.
No hay copia automática instalada todavía.

**Revertir nginx** (volver al acceso con clave básica): restaurar `/root/siffra/nginx-siffra.bak-20261005` en
`/etc/nginx/sites-available/siffra`, `nginx -t && systemctl reload nginx`.
