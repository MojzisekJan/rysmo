import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';
const source = await readFile(new URL('../crates/rysmo_app/src/web_worker.js', import.meta.url), 'utf8');

async function fixture({ missing = false, throws = false } = {}) {
    const workers = [], timers = new Map();
    class Worker {
        constructor(url, options) {
            if (throws) throw Error('worker blocked');
            this.url = String(url); this.options = options; this.stopped = 0; workers.push(this);
        }
        postMessage(data, transfer) { this.data = structuredClone(data, { transfer }); }
        terminate() { this.stopped++; }
    }
    const context = vm.createContext({ Worker, URL, document: {
        baseURI: 'https://example.com/editor/',
        querySelector: () => missing ? null : { content: 'worker-abc/worker.mjs' },
    }, setTimeout: callback => { const id = timers.size + 1; timers.set(id, callback); return id; },
    clearTimeout: id => timers.delete(id) });
    const module = new vm.SourceTextModule(source, { context });
    await module.link(() => {}); await module.evaluate();
    return { api: module.namespace, workers, timers };
}

test('import transfers an owned copy and resolves worker paths under hosted subdirectories', async () => {
    const { api, workers, timers } = await fixture();
    const memory = new Uint8Array([1, 2, 3]);
    const id = api.start_import('model.skp', memory, undefined, false, 48);
    assert.deepEqual([...memory], [1, 2, 3]);
    assert.deepEqual([...workers[0].data.bytes], [1, 2, 3]);
    assert.equal(workers[0].url, 'https://example.com/editor/worker-abc/worker.mjs');
    assert.equal(api.poll_job(id), null);
    workers[0].onmessage({ data: { document: 'model' } });
    assert.equal(api.poll_job(id).document, 'model');
    assert.equal(workers[0].stopped, 1);
    assert.equal(timers.size, 0);
});

test('cancelling a job terminates it and ignores a late result without affecting another job', async () => {
    const { api, workers } = await fixture();
    const old = api.start_export('old', 2017);
    api.cancel_job(old);
    const current = api.start_export('current', 2026);
    workers[0].onmessage({ data: { bytes: [99] } });
    assert.equal(api.poll_job(current), null);
    workers[1].onmessage({ data: { bytes: [7], warnings: '[]' } });
    assert.deepEqual(api.poll_job(current).bytes, [7]);
    assert.match(api.poll_job(old).error, /no longer/);
    assert.equal(workers[0].stopped, 1);
});

test('worker construction, runtime, transfer and timeout failures complete with errors', async () => {
    for (const options of [{ missing: true }, { throws: true }]) {
        const { api, timers } = await fixture(options);
        assert.ok(api.poll_job(api.start_export('{}', 2017)).error);
        assert.equal(timers.size, 0);
    }
    for (const failure of ['runtime', 'message', 'timeout']) {
        const { api, workers, timers } = await fixture();
        const id = api.start_export('{}', 2017);
        if (failure === 'runtime') workers[0].onerror({ message: 'WASM panic', preventDefault() {} });
        if (failure === 'message') workers[0].onmessageerror();
        if (failure === 'timeout') [...timers.values()][0]();
        assert.ok(api.poll_job(id).error);
        assert.equal(workers[0].stopped, 1);
        assert.equal(timers.size, 0);
    }
});

test('native Open isolates errors and cancellation from a newer request', async () => {
    const { api, workers, timers } = await fixture();
    const first = api.start_open('old native file');
    assert.equal(workers[0].data.kind, 'open');
    assert.equal(workers[0].data.document, 'old native file');
    api.cancel_job(first);
    const current = api.start_open('new native file');
    workers[0].onmessage({ data: { document: 'stale result' } });
    assert.equal(api.poll_job(current), null);
    workers[1].onmessage({ data: { error: 'invalid native geometry' } });
    assert.match(api.poll_job(current).error, /invalid native/);
    const valid = api.start_open('valid native file');
    workers[2].onmessage({ data: { document: '[{},{}]' } });
    assert.equal(api.poll_job(valid).document, '[{},{}]');
    assert.equal(timers.size, 0);
    assert.equal(workers[0].stopped, 1);
});

test('worker dispatches native Open after initialization and reports parse failures', async () => {
    const messages = [], calls = [];
    const self = { postMessage: message => messages.push(message) };
    const context = vm.createContext({ self });
    const bridge = new vm.SyntheticModule(['default', 'open_native', 'import_model', 'export_skp'], function () {
        this.setExport('default', async () => { calls.push('init'); });
        this.setExport('open_native', text => {
            calls.push(text);
            if (text === 'invalid') throw Error('invalid geometry');
            return '["document","camera"]';
        });
        this.setExport('import_model', () => { throw Error('wrong operation'); });
        this.setExport('export_skp', () => { throw Error('wrong operation'); });
    }, { context });
    const worker = new vm.SourceTextModule(await readFile(new URL('../crates/rysmo_worker/worker.mjs', import.meta.url), 'utf8'), { context });
    await worker.link(() => bridge); await worker.evaluate();
    await self.onmessage({ data: { kind: 'open', document: 'valid' } });
    assert.deepEqual(calls, ['init', 'valid']);
    assert.equal(messages[0].document, '["document","camera"]');
    await self.onmessage({ data: { kind: 'open', document: 'invalid' } });
    assert.match(messages[1].error, /invalid geometry/);
});
