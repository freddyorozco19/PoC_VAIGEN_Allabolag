// pm2 en el VPS: /root/siffra/ecosystem.config.cjs  →  pm2 start ecosystem.config.cjs && pm2 save
module.exports = {
  apps: [{
    name: "siffra",
    script: "./target/release/siffra-rs",
    cwd: "/root/siffra/siffra-rs",
    interpreter: "none",
    env: {
      PORT: "3011",
      // Cuentas, sesiones y registro de actividad (SQLite). Hacer copia con `siffra-rs backup`.
      SIFFRA_DB: "/root/siffra/data/siffra.db",
      // Detrás de HTTPS: las cookies de sesión llevan la marca Secure.
      SIFFRA_SECURE_COOKIES: "1",
    },
    max_memory_restart: "300M",
    autorestart: true,
    time: true,
  }],
};
