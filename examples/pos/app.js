const $=id=>document.getElementById(id);
const escapeText=s=>String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
const money=n=>Math.round(n).toString().replace(/\B(?=(\d{3})+(?!\d))/g,',');
const products=[
 [1,'Whole milk','1 litre','Dairy',4500,28,'MILK','bottle','#dcebf4','#84aec4'],
 [2,'Sourdough bread','Fresh loaf','Bakery',8500,16,'BAKE','bag','#f3e8da','#c7a37b'],
 [3,'Arabica coffee','250 g','Pantry',24000,12,'BEAN','bag','#e9e0d9','#8a7065'],
 [4,'Free-range eggs','Tray of 6','Dairy',6000,32,'EGGS','box','#f3edde','#cfb877'],
 [5,'Mineral water','500 ml','Drinks',1500,64,'H₂O','bottle','#e0edf0','#82b2bd'],
 [6,'Butter croissant','Freshly baked','Bakery',5000,18,'BAKE','bag','#f6e6d9','#d4a078'],
 [7,'Orange juice','1 litre','Drinks',7500,21,'OJ','bottle','#f8ecd8','#e0b465'],
 [8,'Greek yoghurt','400 g','Dairy',9000,15,'DAIRY','box','#e6e6f2','#9b99c1'],
 [9,'Basmati rice','1 kg','Pantry',8000,40,'RICE','bag','#e6ecdf','#a0b28b'],
 [10,'Dark chocolate','100 g','Snacks',6500,26,'COCOA','box','#ede1e7','#b4869d'],
 [11,'Sea salt crisps','150 g','Snacks',4000,35,'CRISP','bag','#f0e6d9','#c5a16d'],
 [12,'Green tea','20 bags','Pantry',12000,19,'TEA','box','#e3ede5','#8bad94']
].map(([id,name,unit,category,price,stock,label,shape,bg,ink])=>({id,name,unit,category,price,stock,label,shape,bg,ink,barcode:'200000000'+String(id).padStart(3,'0')}));
let cart=[{id:1,qty:2},{id:2,qty:1},{id:3,qty:1}],category='All',view='sale',discount=0,saleNumber=1,nextSaleNumber=2,held=[],sales=[],method='Cash',returnFocus=null;
const product=id=>products.find(p=>p.id===Number(id));
const report=text=>{$('status').textContent=text;};
function totals(){const subtotal=cart.reduce((s,i)=>s+product(i.id).price*i.qty,0),saving=Math.round(subtotal*discount/100);return{subtotal,saving,total:subtotal-saving};}
function renderProducts(){
 const q=$('search').value.trim().toLowerCase(),list=products.filter(p=>(category==='All'||p.category===category)&&(!q||(p.name+' '+p.unit+' '+p.barcode).toLowerCase().includes(q)));
 $('catalog').innerHTML=list.length?list.map(p=>'<button class="product" data-product="'+p.id+'"'+(!p.stock?' disabled':'')+' aria-label="Add '+escapeText(p.name)+'"><div class="art" style="background:'+p.bg+';--ink:'+p.ink+'"><span class="stock">'+p.stock+' in stock</span><div class="package '+p.shape+'">'+p.label+'</div></div><div class="product-info"><span class="product-name">'+p.name+'</span><span class="product-meta">'+p.unit+'</span><div class="product-price"><span>'+money(p.price)+'</span><span class="add">+</span></div></div></button>').join(''):'<div class="empty">No matching products.</div>';
 $('category-title').textContent=category==='All'?'All products':category;$('product-count').textContent=list.length+' products';
 $('categories').innerHTML=['All','Bakery','Dairy','Drinks','Pantry','Snacks'].map(c=>'<button class="category'+(c===category?' active':'')+'" data-category="'+c+'">'+c+'</button>').join('');
}
function renderCart(){
 $('cart-items').innerHTML=cart.length?cart.map(i=>{const p=product(i.id);return'<div class="cart-item"><div class="item-top"><b>'+p.name+'</b><span>'+money(p.price*i.qty)+'</span></div><div class="item-controls"><button data-quantity="-1" data-id="'+i.id+'" aria-label="Reduce '+p.name+'">−</button><span>'+i.qty+'</span><button data-quantity="1" data-id="'+i.id+'" aria-label="Increase '+p.name+'">+</button><span class="unit-price">'+money(p.price)+' each</span><button class="remove" data-remove="'+i.id+'" aria-label="Remove '+p.name+'">×</button></div></div>';}).join(''):'<div class="empty">Your sale starts here.<br>Add a product from the catalog.</div>';
 const t=totals();$('subtotal').textContent=money(t.subtotal);$('discount-value').textContent='− '+money(t.saving);$('total').textContent=money(t.total);$('charge-total').textContent=money(t.total);$('cart-count').textContent=cart.reduce((n,i)=>n+i.qty,0)+' items';$('sale-number').textContent='Sale #'+String(saleNumber).padStart(3,'0');$('checkout').disabled=!cart.length;$('hold').disabled=!cart.length;$('clear').disabled=!cart.length;
}
function add(id){const p=product(id),line=cart.find(i=>i.id===p.id);if((line?.qty||0)>=p.stock)return report('Only '+p.stock+' '+p.name+' available');if(line)line.qty++;else cart.push({id:p.id,qty:1});renderCart();report(p.name+' added');}
function emptyCart(){cart=[];discount=0;$('discount').value='0';$('customer').value='Walk-in customer';renderCart();}
function navigate(next){view=next;document.querySelectorAll('[data-view]').forEach(b=>b.classList.toggle('active',b.dataset.view===view));$('sale-view').classList.toggle('hidden',view!=='sale');$('management-view').classList.toggle('hidden',view==='sale');if(view!=='sale')renderManagement();}
function renderManagement(){
 $('management-title').textContent={orders:'Your open orders.',products:'Everything on your shelves.',reports:'A look at today.'}[view];
 if(view==='orders')$('management').innerHTML=held.length?'<div class="table"><div class="table-row table-head"><span>ORDER</span><span>CUSTOMER</span><span>TOTAL</span><span></span></div>'+held.map((h,i)=>'<div class="table-row"><b>Held sale #'+h.number+'</b><span>'+escapeText(h.customer)+'</span><span>'+money(h.total)+'</span><button class="secondary" data-resume="'+i+'">Resume</button></div>').join('')+'</div>':'<div class="empty">No held sales yet. Use “Hold sale” at checkout.</div>';
 if(view==='products')$('management').innerHTML='<div class="table"><div class="table-row table-head"><span>PRODUCT</span><span>CATEGORY</span><span>PRICE / UGX</span><span>STOCK</span></div>'+products.map(p=>'<div class="table-row"><b>'+p.name+'</b><span>'+p.category+'</span><span>'+money(p.price)+'</span><span>'+p.stock+'</span></div>').join('')+'</div>';
 if(view==='reports')$('management').innerHTML='<div class="stats"><div class="stat"><span>Sales / UGX</span><strong>'+money(sales.reduce((s,r)=>s+r.total,0))+'</strong></div><div class="stat"><span>Transactions</span><strong>'+sales.length+'</strong></div><div class="stat"><span>Items sold</span><strong>'+sales.reduce((s,r)=>s+r.items.reduce((n,i)=>n+i.qty,0),0)+'</strong></div></div><div class="table"><div class="table-row table-head"><span>SALE</span><span>PAYMENT</span><span>TOTAL</span><span></span></div>'+sales.map((s,i)=>'<div class="table-row"><b>Sale #'+s.number+'</b><span>'+s.method+'</span><span>'+money(s.total)+'</span><button class="secondary" data-receipt="'+i+'">Receipt</button></div>').join('')+'</div>';
}
function closeModal(){ $('modal').classList.add('hidden');$('modal').innerHTML='';returnFocus?.focus(); }
function dialog(html){if($('modal').classList.contains('hidden'))returnFocus=document.activeElement;$('modal').innerHTML='<section class="dialog" role="dialog" aria-modal="true" aria-label="Sale dialog">'+html+'</section>';$('modal').classList.remove('hidden');$('modal').querySelector('input, button')?.focus();}
function payment(){if(!cart.length)return;method='Cash';drawPayment();}
function drawPayment(){const t=totals();dialog('<div class="dialog-head"><h2>Complete the sale</h2><button class="close" data-close aria-label="Close">×</button></div><div class="due"><span>AMOUNT DUE · UGX</span><strong>'+money(t.total)+'</strong></div><div class="payment-methods">'+['Cash','Card','Mobile money'].map(m=>'<button data-method="'+m+'" class="'+(method===m?'active':'')+'">'+m+'</button>').join('')+'</div>'+(method==='Cash'?'<label class="field" for="cash">Cash received / UGX</label><input id="cash" type="number" min="0" value="'+t.total+'"><div class="tenders"><button data-tender="'+t.total+'">Exact</button><button data-tender="'+Math.ceil(t.total/10000)*10000+'">'+money(Math.ceil(t.total/10000)*10000)+'</button><button data-tender="'+Math.max(100000,Math.ceil(t.total/50000)*50000)+'">'+money(Math.max(100000,Math.ceil(t.total/50000)*50000))+'</button></div><div class="summary-row"><span>Change</span><b id="change">0</b></div>':'<p class="dialog-note">Preview a '+method.toLowerCase()+' payment. No payment is processed.</p>')+'<div id="payment-error"></div><button class="primary" id="complete">Complete sample sale</button><p class="dialog-note">Demo checkout · No money is collected.</p>');if(method==='Cash')$('cash').focus();}
function complete(){const t=totals(),received=method==='Cash'?Number($('cash').value):t.total;if(!Number.isFinite(received)||!Number.isInteger(received)||received<t.total)return $('payment-error').textContent='Cash received must cover the amount due.';if(cart.some(i=>i.qty>product(i.id).stock))return $('payment-error').textContent='Stock changed. Adjust the quantities before checkout.';const sale={number:saleNumber,customer:$('customer').value,items:cart.map(i=>({...i,name:product(i.id).name,price:product(i.id).price})),...t,method,received,change:received-t.total};cart.forEach(i=>product(i.id).stock-=i.qty);sales.push(sale);saleNumber=nextSaleNumber++;emptyCart();renderProducts();if(view!=='sale')renderManagement();receipt(sale);report('Sample sale completed · '+money(sale.total)+' UGX');}
function receipt(s){dialog('<div class="dialog-head"><h2>Sale complete.</h2><button class="close" data-close aria-label="Close">×</button></div><div class="receipt-meta">Northstar · Central store<br>Receipt #'+String(s.number).padStart(3,'0')+' · '+escapeText(s.customer)+'</div>'+s.items.map(i=>'<div class="receipt-line"><span>'+i.qty+' × '+i.name+'</span><b>'+money(i.qty*i.price)+'</b></div>').join('')+'<div class="summary-row"><span>Discount</span><span>− '+money(s.saving)+'</span></div><div class="total-row"><span>Total / UGX</span><strong>'+money(s.total)+'</strong></div><div class="receipt-line"><span>'+s.method+'</span><span>'+money(s.received)+'</span></div><div class="receipt-line"><span>Change</span><span>'+money(s.change)+'</span></div><button class="primary" data-close>Back to counter</button><p class="dialog-note">Sample receipt · Thank you for shopping with us.</p>');}
$('search').addEventListener('input',renderProducts);$('search').addEventListener('keydown',e=>{if(e.key==='Enter'){const p=products.find(p=>p.barcode===$('search').value.trim());if(p){add(p.id);$('search').value='';renderProducts();}else report('Scan an exact barcode or click a search result');}});
$('discount').addEventListener('input',()=>{discount=Math.max(0,Math.min(100,Number($('discount').value)||0));renderCart();});$('discount').addEventListener('change',()=>{$('discount').value=String(discount);});
$('checkout').addEventListener('click',payment);$('clear').addEventListener('click',()=>{emptyCart();report('Sale cleared');});$('new-sale').addEventListener('click',()=>{if(cart.length){holdSale();}else{emptyCart();}navigate('sale');$('search').focus();});
function holdSale(){if(!cart.length)return;held.push({number:saleNumber,cart:cart.map(i=>({...i})),discount,customer:$('customer').value,total:totals().total});saleNumber=nextSaleNumber++;emptyCart();report('Sale held · Find it in Orders');}
$('hold').addEventListener('click',holdSale);
document.addEventListener('click',e=>{
 const p=e.target.closest('[data-product]');if(p)return add(p.dataset.product);
 const c=e.target.closest('[data-category]');if(c){category=c.dataset.category;return renderProducts();}
 const q=e.target.closest('[data-quantity]');if(q){const i=cart.find(i=>i.id===Number(q.dataset.id));if(Number(q.dataset.quantity)>0)return add(i.id);if(--i.qty===0)cart=cart.filter(n=>n!==i);return renderCart();}
 const r=e.target.closest('[data-remove]');if(r){cart=cart.filter(i=>i.id!==Number(r.dataset.remove));return renderCart();}
 const n=e.target.closest('[data-view]');if(n)return navigate(n.dataset.view);
 const resume=e.target.closest('[data-resume]');if(resume){const index=Number(resume.dataset.resume),h=held.splice(index,1)[0];if(cart.length)holdSale();cart=h.cart;discount=h.discount;$('discount').value=String(discount);$('customer').value=h.customer;saleNumber=h.number;navigate('sale');renderCart();return report('Held sale resumed');}
 const rec=e.target.closest('[data-receipt]');if(rec)return receipt(sales[Number(rec.dataset.receipt)]);
 if(e.target.closest('[data-close]'))return closeModal();
 const m=e.target.closest('[data-method]');if(m){method=m.dataset.method;return drawPayment();}
 const tender=e.target.closest('[data-tender]');if(tender){$('cash').value=tender.dataset.tender;return updateChange();}
 if(e.target.closest('#complete'))complete();
});
function updateChange(){$('change').textContent=money(Math.max(0,Number($('cash').value)-totals().total));$('payment-error').textContent='';}
$('modal').addEventListener('input',e=>{if(e.target.id==='cash')updateChange();});
document.addEventListener('keydown',e=>{
 if(!$('modal').classList.contains('hidden')){if(e.key==='Escape'){e.preventDefault();closeModal();}else if(e.key==='Tab'){const items=$('modal').querySelectorAll('button,input');const i=items.indexOf(document.activeElement);e.preventDefault();items[(i+(e.shiftKey?-1:1)+items.length)%items.length]?.focus();}return;}
 if(e.key==='F3'){e.preventDefault();navigate('sale');$('search').focus();}if(e.key==='F8'){e.preventDefault();payment();}
});
renderProducts();renderCart();
