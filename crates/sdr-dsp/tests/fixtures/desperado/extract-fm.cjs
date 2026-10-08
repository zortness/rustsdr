const fs = require('fs');
const input = fs.readFileSync(process.argv[2] || 'target/desperado-review/crates/fmradio/tests/data/rtlsdr_20210215_103300000_1102500_cu8.bin');
const rate = 1102500, outputRate = 240000, offset = 200000, radius = 80;
const count = input.length / 2, re = new Float32Array(count), im = new Float32Array(count);
for (let n=0;n<count;n++) {
  const phase = -2*Math.PI*offset*n/rate, c=Math.cos(phase), s=Math.sin(phase);
  const i=(input[2*n]-127.5)/127.5, q=(input[2*n+1]-127.5)/127.5;
  re[n]=i*c-q*s; im[n]=q*c+i*s;
}
const taps = Array.from({length:32},(_,phase)=>{
  const frac=phase/32, coeff=[];
  for(let k=-radius;k<=radius;k++) {
    const x=k-frac, w=0.42+0.5*Math.cos(Math.PI*x/(radius+1))+0.08*Math.cos(2*Math.PI*x/(radius+1));
    coeff.push((Math.abs(x)<1e-10 ? 2*95000/rate : Math.sin(2*Math.PI*95000/rate*x)/(Math.PI*x))*w);
  }
  const sum=coeff.reduce((a,b)=>a+b,0);return coeff.map(v=>v/sum);
});
const out=Buffer.alloc(Math.floor(count*outputRate/rate)*4);
let previousI=0, previousQ=0;
for(let n=0;n<out.length/4;n++) {
  const pos=n*147/32, base=Math.floor(pos), h=taps[(n*147)%32];
  let i=0,q=0;
  for(let k=-radius;k<=radius;k++) {
    const j=base+k;if(j>=0&&j<count) {i+=re[j]*h[k+radius];q+=im[j]*h[k+radius];}
  }
  const mpx=Math.atan2(q*previousI-i*previousQ,i*previousI+q*previousQ)*outputRate/(2*Math.PI*75000);
  out.writeFloatLE(mpx,n*4);previousI=i;previousQ=q;
}
fs.writeFileSync('crates/sdr-dsp/tests/fixtures/desperado/fip-mpx.f32',out);
