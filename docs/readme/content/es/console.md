Las pruebas se ejecutan desde tu terminal o tu agente. Para revisarlas después, `qualyx serve` te da una pequeña consola web:

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="assets/screenshots/console-es-dark.png" /><img src="assets/screenshots/console-es-light.png" alt="Consola de Qualyx: ocho ejecuciones de un proyecto, seis correctas y dos con fallos, con el enlace al reporte en cada fila" width="100%" /></picture></p>

- **El historial de un vistazo:** cuántas ejecuciones pasaron, fallaron o siguen en curso, con búsqueda por identificador y filtro por estado.
- **La evidencia, no un resumen:** abre el `report.md` de cualquier ejecución tal como lo escribió Qualyx.
- **Claro u oscuro, español o inglés:** sigue a tu sistema y a tu navegador, y puedes cambiar ambos.
- **Solo lectura a propósito:** nunca inicia ni cancela nada, así que dejarla abierta no tiene riesgo.

```sh
export QUALYX_API_TOKEN="$(openssl rand -hex 24)"   # 32+ caracteres imprimibles
qualyx serve --root /ruta/a/proyectos               # luego abre http://127.0.0.1:7878
```

Escribe la ruta de un proyecto dentro de `--root` y el token. El token solo vive en esa pestaña y se borra al recargar. La consola viene dentro del binario `qualyx`, así que no hay nada más que instalar ni levantar: ni Node.js ni otro servidor.
