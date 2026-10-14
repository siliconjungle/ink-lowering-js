function __versionWord(v){return typeof v==='number'?BigInt(v):v;}
function __storedCommit(meta,v){return meta.compactCommits&&v<=9007199254740991n?Number(v):v;}
function __nextCommit(v){return typeof v==='number'&&v<9007199254740991?v+1:BigInt(v)+1n;}
// Private column event log. Public records and portable event ordering are unchanged.
// Private event storage. Public events and portable snapshots keep their ordinary
// ordered records. Chunks bound array growth and retain no per-event record.
class __EventColumns {
 constructor(channels,compact=false){this.channels=channels;this.compact=compact;this.commits=[];this.values=[];this.positions=null;this.channelValues=channels.length===1?null:[];}
 get length(){return this.commits.length;}
 set length(n){if(!Number.isSafeInteger(n)||n<0||n>this.length)throw Error('event log truncate');this.commits.length=n;this.values.length=n;if(this.positions)this.positions.length=n;if(this.channelValues)this.channelValues.length=n;}
 append(commit,position,channel,value){const i=this.commits.length;this.commits.push(commit);this.values.push(value);if(this.channelValues)this.channelValues.push(channel);if(position!==0n&&!this.positions)this.positions=new Array(i).fill(0n);if(this.positions)this.positions.push(position);}
 push(e){this.append(this.compact&&e.commit<=9007199254740991n?Number(e.commit):e.commit,e.position,e.channel,e.value);return this.length;}
 at(i){if(i<0)i+=this.length;if(i<0||i>=this.length)return undefined;return {commit:__versionWord(this.commits[i]),position:this.positions?this.positions[i]:0n,channel:this.channelValues?this.channelValues[i]:this.channels[0],value:this.values[i]};}
 *[Symbol.iterator](){for(let i=0;i<this.length;i++)yield this.at(i);}
 map(f){const out=new Array(this.length);for(let i=0;i<this.length;i++)out[i]=f(this.at(i),i);return out;}
 filter(f){const out=new __EventColumns(this.channels,this.compact);for(let i=0;i<this.length;i++){const e=this.at(i);if(f(e,i))out.push(e);}return out;}
}
function __eventLog(meta,events=[]){
 if(!meta.eventColumns)return meta.compactCommits?events.map(e=>({...e,commit:__storedCommit(meta,e.commit)})):events;
 const log=new __EventColumns(meta.events.map(e=>e.name),meta.compactCommits);for(const e of events)log.append(__storedCommit(meta,e.commit),e.position,e.channel,e.value);return log;
}
