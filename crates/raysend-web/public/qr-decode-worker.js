let decodeFn = null;

self.onmessage = async (event) => {
  const msg = event.data || {};
  if (msg.op === "boot") {
    try {
      const mod = await import(msg.js);
      const init = mod.default || mod.init;
      if (typeof init !== "function") {
        throw new Error("wasm init missing");
      }
      const wasmUrls = Array.isArray(msg.wasm) ? msg.wasm : [msg.wasm];
      let booted = false;
      let lastErr = null;
      try {
        await init();
        booted = true;
      } catch (err) {
        lastErr = err;
      }
      for (const url of wasmUrls) {
        if (booted || !url) {
          break;
        }
        try {
          await init({ module_or_path: url });
          booted = true;
        } catch (err) {
          lastErr = err;
        }
      }
      if (!booted) {
        throw lastErr || new Error("wasm init failed");
      }
      decodeFn = mod.raysend_decode_luma;
      if (typeof decodeFn !== "function") {
        throw new Error("raysend_decode_luma missing");
      }
      self.postMessage({ op: "ready" });
    } catch (err) {
      self.postMessage({
        op: "fail",
        err: String(err && err.message ? err.message : err),
      });
    }
    return;
  }

  if (msg.op === "decode") {
    if (typeof decodeFn !== "function") {
      self.postMessage({ op: "done", id: msg.id, x: msg.x, y: msg.y, err: "not ready" });
      return;
    }
    try {
      const luma = new Uint8Array(msg.luma);
      const out = decodeFn(
        msg.width,
        msg.height,
        luma,
        !!msg.discover,
        msg.modules || 0,
        msg.x0 || 0,
        msg.y0 || 0,
        msg.x1 || 0,
        msg.y1 || 0,
        msg.x2 || 0,
        msg.y2 || 0,
        msg.x3 || 0,
        msg.y3 || 0
      );
      self.postMessage({
        op: "done",
        id: msg.id,
        x: msg.x,
        y: msg.y,
        payloads: out.payloads,
        regions: out.regions,
        hints: out.hints,
        tracked: !!out.tracked,
        had_hint: !!out.had_hint,
        discover: !!out.discover,
      });
    } catch (err) {
      self.postMessage({
        op: "done",
        id: msg.id,
        x: msg.x,
        y: msg.y,
        err: String(err && err.message ? err.message : err),
      });
    }
  }
};
