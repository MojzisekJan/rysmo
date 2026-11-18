// Manual browser profiling. Requires Playwright and a staged, locally served probe.
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import {execFileSync} from 'node:child_process';
import {resolve} from 'node:path';
const {chromium} = await import(process.env.RYSMO_PLAYWRIGHT || 'playwright');
const base = process.argv[2] || 'http://127.0.0.1:8109';
const output = resolve(process.argv[3] || 'target/web-profile-results');
const cases = (process.env.RYSMO_PROFILE_CASES || 'flat,textured,raw').split(',');
await mkdir(output,{recursive:true});
const summary = values => {
  const sorted = values.filter(Number.isFinite).sort((a,b)=>a-b);
  return sorted.length ? {count:sorted.length,median:sorted[Math.floor(sorted.length/2)],p95:sorted[Math.ceil(sorted.length*.95)-1],maximum:sorted.at(-1)} : null;
};
const cadence = (samples, begin, end) => {
  const times=samples.filter(sample=>sample.stage==='editor_update' && sample.time>=begin && sample.time<=end).map(sample=>sample.time);
  return times.length>1 ? {updates:times.length,fps:(times.length-1)*1000/(times.at(-1)-times[0]),intervalMilliseconds:summary(times.slice(1).map((time,i)=>time-times[i]))} : null;
};
const results = [];
for (const name of cases) {
  const chromiumArgs=['--no-sandbox','--enable-precise-memory-info',...(process.env.RYSMO_SOFTWARE_GL
    ? ['--use-angle=swiftshader','--enable-unsafe-swiftshader'] : ['--enable-gpu','--use-angle=gl'])];
  const browser = await chromium.launch({executablePath:process.env.RYSMO_CHROMIUM || undefined,headless:true,
    ignoreDefaultArgs:process.env.RYSMO_SOFTWARE_GL ? [] : ['--enable-unsafe-swiftshader'],args:chromiumArgs});
  const context = await browser.newContext({viewport:{width:1280,height:800},deviceScaleFactor:1});
  const page = await context.newPage();
  const errors = [], consoleErrors = [], consoleWarnings = [];
  page.on('pageerror',error=>errors.push(String(error)));
  page.on('console',message=>{if(message.type()==='error')consoleErrors.push(message.text());if(message.type()==='warning')consoleWarnings.push(message.text());});
  await page.addInitScript(()=>{
    const original = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function(...args) {
      const result = original.apply(this,args);
      if (args[0]==='webgl2' && this.id==='rysmo') window.rysmoProfileGL=result;
      return result;
    };
    window.rysmoRafIntervals=[];window.rysmoLongTasks=[];
    let last;
    function frame(time){if(last!=null){const samples=window.rysmoRafIntervals;if(samples.length>=3600)samples.shift();samples.push({time,duration:time-last});}last=time;requestAnimationFrame(frame);}
    requestAnimationFrame(frame);
    try{new PerformanceObserver(list=>{for(const entry of list.getEntries()){if(window.rysmoLongTasks.length<512)window.rysmoLongTasks.push({start:entry.startTime,duration:entry.duration});}}).observe({type:'longtask',buffered:true});}catch{}
  });
  const cdp = await context.newCDPSession(page);await cdp.send('Performance.enable');
  const system = await browser.newBrowserCDPSession();
  const gpuInfo = await system.send('SystemInfo.getInfo').catch(error=>({error:String(error)}));
  const take = () => page.evaluate(()=>{
    const p=window.rysmoPerformance;
    return {time:performance.now(),samples:p?.samples.slice() || [],milestones:p?.milestones.slice() || [],dropped:p?.dropped || 0,gpu:p?.gpu,
      wasmBytes:p?.memory?.buffer.byteLength ?? null,jsHeapBytes:performance.memory?.usedJSHeapSize ?? null,
      raf:window.rysmoRafIntervals.slice(),longTasks:window.rysmoLongTasks.slice(),
      editor:document.getElementById('web-diagnostics')?.dataset.editor || null,
      renderer:window.rysmoProfileGL ? (()=>{const gl=window.rysmoProfileGL,ext=gl.getExtension('WEBGL_debug_renderer_info');return ext ? gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);})() : null};
  });
  let result;
  try {
    await page.goto(`${base}/index.html?case=profile-${name}&diagnostics`,{waitUntil:'domcontentloaded',timeout:120000});
    await page.waitForFunction(()=>window.rysmoPerformance?.milestones.some(s=>s.stage==='model_installed'),null,{timeout:180000});
    let visible=null, pixels=0;
    const deadline=Date.now()+180000;
    const firstPath=resolve(output,`${name}-first.png`);
    while(Date.now()<deadline){
      const state=await take();
      const installed=state.milestones.find(s=>s.stage==='model_installed');
      if(state.milestones.some(s=>s.stage==='surfaces_ready' && s.generation===installed.generation)){
        await page.screenshot({path:firstPath,timeout:120000});
        pixels=Number(execFileSync('python3',['-c',`from PIL import Image
import sys
im=Image.open(sys.argv[1]).convert('RGB');w,h=im.size
roi=im.crop((int(w*.3),int(h*.28),int(w*.78),int(h*.78)))
print(sum(1 for r,g,b in roi.getdata() if (r>g*1.35 and b>g*1.25 and r>60) or (r>g*1.02 and r>b*1.10 and r>70)))`,firstPath],{encoding:'utf8'}).trim());
        if(pixels>2000){visible=(await take()).time;break;}
      }
      await page.waitForTimeout(200);
    }
    if(visible==null)throw Error('No model-coloured surfaces confirmed in viewport screenshot');
    await page.waitForFunction(()=>{
      const milestones=window.rysmoPerformance?.milestones || [];
      const installed=milestones.find(s=>s.stage==='model_installed');
      return installed && milestones.some(s=>s.stage==='surfaces_uploaded' && s.generation===installed.generation);
    },null,{timeout:180000});
    const cold=await take();console.log(name,"surfaces visible");
    await page.mouse.move(640,420);
    await page.waitForTimeout(300);
    for(let i=0;i<90;i++){await page.mouse.move(420+(i*7)%460,300+(i*11)%280);await page.waitForTimeout(16);}
    const picked=await take();console.log(name,"pointer phase complete");
    const update = async(action)=>{
      const serial=await page.evaluate(()=>window.rysmoPerformance?.samples.findLast(sample=>sample.stage==='editor_update')?.time || 0);
      await action();
      await page.waitForFunction(value=>(window.rysmoPerformance?.samples.findLast(sample=>sample.stage==='editor_update')?.time || 0)>value,serial,{timeout:30000});
    };
    let editAttempted=0;
    if(name!=='raw'){
      await update(()=>page.mouse.click(620,420));await update(()=>page.keyboard.press('Control+a'));
      const attempts=Number(process.env.RYSMO_PROFILE_EDITS || 24);
      for(let i=0;i<attempts;i++){
        const direction=i%2===0 ? 1 : -1;
        await update(()=>page.keyboard.press('m'));
        await update(()=>page.mouse.click(620,420));
        await update(()=>page.mouse.move(620+20*direction,420+10*direction));
        await update(()=>page.mouse.click(620+20*direction,420+10*direction));
        await update(()=>page.keyboard.press('Space'));editAttempted++;
        await page.waitForTimeout(80);
        if((i+1)%4===0)console.log(name,"edits",i+1);
      }
      await update(()=>page.keyboard.press('Control+z'));await update(()=>page.keyboard.press('Control+Shift+z'));
    }
    const editEnd=await take();
    await page.waitForTimeout(2000);
    const sustained=await take();
    await page.screenshot({path:resolve(output,`${name}-final.png`),timeout:120000});
    const stages={};for(const sample of sustained.samples)(stages[sample.stage] ||= []).push(sample.milliseconds);
    result={name,status:'completed',firstVisibleConfirmationUpperBoundMs:visible,firstVisibleAfterModelInstallUpperBoundMs:visible-cold.milestones.find(sample=>sample.stage==='model_installed').time,visiblePixels:pixels,cold,
      firstPointer: picked.samples.find(s=>s.stage==='pointer_event' && s.time>=cold.time) || null,
      editAttempted,undoDepth:Number(/undo (\d+)/.exec(sustained.editor || '')?.[1] || 0),
      editVerified:editAttempted===0 ? null : Number(/undo (\d+)/.exec(sustained.editor || '')?.[1] || 0)>=editAttempted,
      sustained,phaseMilliseconds:Object.fromEntries(Object.entries(stages).map(([key,values])=>[key,summary(values)])),
      phaseBoundaries:{coldEnd:cold.time,pointerEnd:picked.time,editEnd:editEnd.time,idleEnd:sustained.time},
      pointerEditorCadence:cadence(picked.samples,cold.time,picked.time),
      editingEditorCadence:cadence(editEnd.samples,picked.time,editEnd.time),
      pointerRafMilliseconds:summary(picked.raf.filter(sample=>sample.time>=cold.time && sample.time<=picked.time).map(sample=>sample.duration)),
      rafMilliseconds:summary(sustained.raf.map(sample=>sample.duration)),performanceMetrics:await cdp.send('Performance.getMetrics'),gpuInfo};
    const processes=await system.send('SystemInfo.getProcessInfo');
    result.processMemory=[];
    for(const process of processes.processInfo){
      try{const status=await readFile(`/proc/${process.id}/status`,'utf8');result.processMemory.push({type:process.type,pid:process.id,
        rssKiB:Number(/^VmRSS:\s+(\d+)/m.exec(status)?.[1] || 0),peakRssKiB:Number(/^VmHWM:\s+(\d+)/m.exec(status)?.[1] || 0)});}catch{}
    }
  }catch(error){result={name,status:'failed',error:String(error),state:await take().catch(()=>null),gpuInfo};}
  result.chromiumArgs=chromiumArgs;
  result.browserVersion=browser.version();
  result.browserEnvironment=await page.evaluate(()=>({userAgent:navigator.userAgent,hardwareConcurrency:navigator.hardwareConcurrency,deviceMemoryGiB:navigator.deviceMemory,crossOriginIsolated})).catch(()=>null);
  result.pageErrors=errors;result.consoleErrors=consoleErrors;result.consoleWarnings=consoleWarnings;
  result.gpuTimingVerified=!!result.sustained?.gpu?.supported && !result.sustained?.gpu?.error && !!result.phaseMilliseconds?.gpu_elapsed?.count;
  results.push(result);
  await writeFile(resolve(output,`${name}.json`),JSON.stringify(result,null,2)+'\n');
  await browser.close();console.log(name,result.status,result.firstVisibleConfirmationUpperBoundMs ?? result.error);
}
await writeFile(resolve(output,'results.json'),JSON.stringify({date:new Date().toISOString(),base,
  caveat:'Manual localhost browser measurements. Screenshot confirmation is an upper bound and includes polling/capture. RAF cadence is separate from editor updates. GPU queries may be unsupported; renderer identity must accompany timings. RSS sums double-count shared memory; WASM buffer size is allocated capacity, not live bytes.',results},null,2)+'\n');
if(results.some(result=>result.status!=='completed' || result.pageErrors.length || result.consoleErrors.length || result.editVerified===false || (!process.env.RYSMO_SOFTWARE_GL && !result.gpuTimingVerified)))process.exitCode=1;
