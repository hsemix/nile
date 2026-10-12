import { createStore } from './store.js';

const store = createStore([
  { text: 'Write the JSX transform', done: true },
  { text: 'Load ES modules', done: true },
  { text: 'Port a Sciter app', done: false },
]);

// A function component: called as Todo(props, children).
const Todo = ({ item, index }) => (
  <li class={{ todo: true, done: item.done }} data-index={index} key={index}>
    <span class="box">{item.done ? '✓' : ''}</span>
    <span class="label">{item.text}</span>
  </li>
);

function render() {
  $('#list').content(store.items.map((item, index) => <Todo item={item} index={index} />));
  $('#count').textContent = `${store.remaining} left`;
}

// Sciter-style delegation: one handler for every current and future <li>.
$('#list').on('click', 'li.todo', function (evt, li) {
  store.toggle(+li.getAttribute('data-index'));
});

$('#add').on('click', () => {
  const n = store.items.length + 1;
  store.add('New task ' + n);
});

store.subscribe(render);
render();
console.log('todo app ready:', $$('li.todo').length, 'items');
