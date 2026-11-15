// Static visual test only; no API, publishing, launches, or game data.
function node(tag,text,cls){const e=document.createElement(tag);if(text!==undefined)e.textContent=text;if(cls)e.className=cls;return e;}
function card(size,title,tags){
  const game=node('article',undefined,'game review-'+size);game.inert=true;
  const art=node('div',undefined,'game-art');art.setAttribute('aria-hidden','true');
  const meta=node('div',undefined,'game-meta');meta.append(node('p','Single player','game-mode'));
  const categories=node('div',undefined,'game-tags');for(const tag of tags)categories.append(node('button',tag,'tag-chip'));meta.append(categories);
  const dock=node('div',undefined,'game-dock'),heading=node('div',undefined,'game-heading');heading.append(node('h2',title));
  const creator=node('button',undefined,'creator');creator.setAttribute('aria-label','Example creator portrait');creator.append(node('span','FD','avatar tone-0'));heading.append(creator);
  const actions=node('div',undefined,'actions'),menu=node('details',undefined,'play-menu');menu.append(node('summary','▾'));actions.append(node('button','Play'),menu);
  dock.append(heading,actions);game.append(art,meta,dock);return game;
}
for(const [size,label]of [['small','284 × 380 · narrow phone'],['medium','368 × 388 · desktop tile'],['large','600 × 632 · large card']]){
  const figure=node('figure',undefined,'review-sample');figure.append(node('figcaption',label),card(size,'Firefly Drift',['Chill','Explore']));document.getElementById('review-cards').append(figure);
}
document.getElementById('stress-card').append(card('small','Firefly Drift: A Very Long Example Title',['Chill','Explore','Adventure','Arcade','Single player']));
