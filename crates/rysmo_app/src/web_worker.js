const jobs = new Map();
let nextId = 1;

function start(data, transfers = []) {
    const id = nextId++;
    const job = { worker: null, result: null, timer: null };
    jobs.set(id, job);
    const finish = (result) => {
        if (!jobs.has(id) || job.result !== null) return;
        job.result = result;
        clearTimeout(job.timer);
        job.worker?.terminate();
        job.worker = null;
    };
    try {
        const path = document.querySelector('meta[name="rysmo-worker"]')?.content;
        if (!path) throw new Error('Conversion worker is missing. Rebuild or reload the editor.');
        job.worker = new Worker(new URL(path, document.baseURI), { type: 'module' });
        job.worker.onmessage = ({ data }) => finish(data);
        job.worker.onerror = (event) => {
            event.preventDefault();
            finish({ error: event.message || 'Conversion worker failed.' });
        };
        job.worker.onmessageerror = () => finish({ error: 'Cannot receive conversion result.' });
        job.timer = setTimeout(() => finish({ error: 'Model conversion timed out.' }), 180000);
        job.worker.postMessage(data, transfers);
    } catch (error) {
        finish({ error: String(error) });
    }
    return id;
}

export function start_import(name, bytes, scale, zUp, segments) {
    // wasm-bindgen lends a view into WASM memory; transfer only an owned copy.
    const owned = bytes.slice();
    return start({ kind: 'import', name, bytes: owned, scale, zUp, segments }, [owned.buffer]);
}

export function start_export(document, version) {
    return start({ kind: 'export', document, version });
}

export function poll_job(id) {
    const job = jobs.get(id);
    if (!job) return { error: 'Conversion job is no longer available.' };
    if (job.result === null) return null;
    jobs.delete(id);
    return job.result;
}

export function cancel_job(id) {
    const job = jobs.get(id);
    if (!job) return;
    jobs.delete(id);
    clearTimeout(job.timer);
    job.worker?.terminate();
}

export function start_open(document) {
    return start({ kind: 'open', document });
}
