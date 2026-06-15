const __vite__mapDeps=(i,m=__vite__mapDeps,d=(m.f||(m.f=["_astro/mermaid.core.B7ni1QoM.js","_astro/preload-helper.DZfPmCzu.js"])))=>i.map(i=>d[i]);
import{_ as S}from"./preload-helper.DZfPmCzu.js";const C={},k=new Set,u=new WeakSet;let h=!0,w,y=!1;function x(e){y||(y=!0,h??=!1,w??="hover",M(),O(),P(),_())}function M(){for(const e of["touchstart","mousedown"])document.addEventListener(e,r=>{const a=r.target.closest("a");m(a,"tap")&&l(a.href,{ignoreSlowConnection:!0})},{passive:!0})}function O(){let e;document.body.addEventListener("focusin",t=>{const n=t.target.closest("a");m(n,"hover")&&r(n.href)},{passive:!0}),document.body.addEventListener("focusout",a,{passive:!0}),g(()=>{for(const t of document.getElementsByTagName("a"))u.has(t)||m(t,"hover")&&(u.add(t),t.addEventListener("mouseenter",n=>r(n.currentTarget.href),{passive:!0}),t.addEventListener("mouseleave",a,{passive:!0}))});function r(t){e&&clearTimeout(e),e=setTimeout(()=>{l(t)},80)}function a(){e&&(clearTimeout(e),e=0)}}function P(){let e;g(()=>{for(const r of document.getElementsByTagName("a"))u.has(r)||m(r,"viewport")&&(u.add(r),e??=I(),e.observe(r))})}function I(){const e=new WeakMap;return new IntersectionObserver((r,a)=>{for(const t of r){const n=t.target,o=e.get(n);t.isIntersecting?(o&&clearTimeout(o),e.set(n,setTimeout(()=>{a.unobserve(n),e.delete(n),l(n.href)},300))):o&&(clearTimeout(o),e.delete(n))}})}function _(){g(()=>{for(const e of document.getElementsByTagName("a"))m(e,"load")&&l(e.href)})}function l(e,r){e=e.replace(/#.*/,"");const a=r?.ignoreSlowConnection??!1;if(N(e,a))if(k.add(e),document.createElement("link").relList?.supports?.("prefetch")){const t=document.createElement("link");t.rel="prefetch",t.setAttribute("href",e),document.head.append(t)}else{const t=new Headers;for(const[n,o]of Object.entries(C))t.set(n,o);fetch(e,{priority:"low",headers:t})}}function N(e,r){if(!navigator.onLine||!r&&E())return!1;try{const a=new URL(e,location.href);return location.origin===a.origin&&(location.pathname!==a.pathname||location.search!==a.search)&&!k.has(e)}catch{}return!1}function m(e,r){if(e?.tagName!=="A")return!1;const a=e.dataset.astroPrefetch;return a==="false"?!1:r==="tap"&&(a!=null||h)&&E()?!0:a==null&&h||a===""?r===w:a===r}function E(){if("connection"in navigator){const e=navigator.connection;return e.saveData||/2g/.test(e.effectiveType)}return!1}function g(e){e();let r=!1;document.addEventListener("astro:page-load",()=>{if(!r){r=!0;return}e()})}const i=(...e)=>console.log("[astro-mermaid]",...e),L=(...e)=>console.error("[astro-mermaid]",...e),T=()=>document.querySelectorAll("pre.mermaid").length>0;let c=null;async function B(){return c||(i("Loading mermaid.js..."),c=S(()=>import("./mermaid.core.B7ni1QoM.js").then(e=>e.bn),__vite__mapDeps([0,1])).then(async({default:e})=>{const r=[];if(r&&r.length>0){i("Registering",r.length,"icon packs");const a=r.map(t=>({name:t.name,loader:()=>fetch(t.url).then(n=>n.json())}));await e.registerIconPacks(a)}return e}).catch(e=>{throw L("Failed to load mermaid:",e),c=null,e}),c)}const f={startOnLoad:!1,theme:"default"},D={light:"default",dark:"dark"};async function p(){i("Initializing mermaid diagrams...");const e=document.querySelectorAll("pre.mermaid");if(i("Found",e.length,"mermaid diagrams"),e.length===0)return;const r=await B();let a=f.theme;{const t=document.documentElement.getAttribute("data-theme"),n=document.body.getAttribute("data-theme");a=D[t||n]||f.theme,i("Using theme:",a,"from",t?"html":"body")}r.initialize({...f,theme:a,gitGraph:{mainBranchName:"main",showCommitLabel:!0,showBranches:!0,rotateCommitLabel:!0}});for(const t of e){if(t.hasAttribute("data-processed"))continue;t.hasAttribute("data-diagram")||t.setAttribute("data-diagram",t.textContent||"");const n=t.getAttribute("data-diagram")||"",o="mermaid-"+Math.random().toString(36).slice(2,11);i("Rendering diagram:",o);try{const d=document.getElementById(o);d&&d.remove();const{svg:s}=await r.render(o,n);t.innerHTML=s,t.setAttribute("data-processed","true"),i("Successfully rendered diagram:",o)}catch(d){L("Mermaid rendering error for diagram:",o,d);const s=document.createElement("div");s.style.cssText="color: red; padding: 1rem; border: 1px solid red; border-radius: 0.5rem;";const b=document.createElement("strong");b.textContent="Error rendering diagram:";const v=document.createElement("span");v.textContent=" "+(d.message||"Unknown error"),s.appendChild(b),s.appendChild(v),t.textContent="",t.appendChild(s),t.setAttribute("data-processed","true")}}}T()?(i("Mermaid diagrams detected on initial load"),p()):i("No mermaid diagrams found on initial load");{const e=new MutationObserver(r=>{for(const a of r)a.type==="attributes"&&a.attributeName==="data-theme"&&(document.querySelectorAll("pre.mermaid[data-processed]").forEach(t=>{t.removeAttribute("data-processed")}),p())});e.observe(document.documentElement,{attributes:!0,attributeFilter:["data-theme"]}),e.observe(document.body,{attributes:!0,attributeFilter:["data-theme"]})}document.addEventListener("astro:after-swap",()=>{i("View transition detected"),T()&&p()});const A=document.createElement("style");A.textContent=`
            /* Prevent layout shifts by setting minimum height */
            pre.mermaid {
              display: flex;
              justify-content: center;
              align-items: center;
              margin: 2rem 0;
              padding: 1rem;
              background-color: transparent;
              border: none;
              overflow: auto;
              min-height: 200px; /* Prevent layout shift */
              position: relative;
            }
            
            /* Loading state with skeleton loader */
            pre.mermaid:not([data-processed]) {
              background: linear-gradient(90deg, #f0f0f0 25%, #e0e0e0 50%, #f0f0f0 75%);
              background-size: 200% 100%;
              animation: shimmer 1.5s infinite;
            }
            
            /* Dark mode skeleton loader */
            [data-theme="dark"] pre.mermaid:not([data-processed]) {
              background: linear-gradient(90deg, #2a2a2a 25%, #3a3a3a 50%, #2a2a2a 75%);
              background-size: 200% 100%;
            }
            
            @keyframes shimmer {
              0% {
                background-position: -200% 0;
              }
              100% {
                background-position: 200% 0;
              }
            }
            
            /* Show processed diagrams with smooth transition */
            pre.mermaid[data-processed] {
              animation: none;
              background: transparent;
              min-height: auto; /* Allow natural height after render */
            }
            
            /* Ensure responsive sizing for mermaid SVGs */
            pre.mermaid svg {
              max-width: 100%;
              height: auto;
            }
            
            /* Optional: Add subtle background for better visibility */
            @media (prefers-color-scheme: dark) {
              pre.mermaid[data-processed] {
                background-color: rgba(255, 255, 255, 0.02);
                border-radius: 0.5rem;
              }
            }
            
            @media (prefers-color-scheme: light) {
              pre.mermaid[data-processed] {
                background-color: rgba(0, 0, 0, 0.02);
                border-radius: 0.5rem;
              }
            }
            
            /* Respect user's color scheme preference */
            [data-theme="dark"] pre.mermaid[data-processed] {
              background-color: rgba(255, 255, 255, 0.02);
              border-radius: 0.5rem;
            }
            
            [data-theme="light"] pre.mermaid[data-processed] {
              background-color: rgba(0, 0, 0, 0.02);
              border-radius: 0.5rem;
            }
          `;document.head.appendChild(A);x();
