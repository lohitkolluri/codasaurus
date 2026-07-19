// Test file for Codasaurus review
// Contains intentional issues for testing

import { useState, useEffect } from 'react';
import { Chart } from 'chart.js'; // chart.js is not declared in package.json (phantom dep)
import { magic } from 'holysuperlibrary'; // this package doesn't exist on npm (hallucinated import)

const API_SECRET = 'sk-live-a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6';

// TODO: implement error handling for API calls
function fetchUserData(userId) {
  return fetch(`/api/users/${userId}`)
    .then(res => res.json());
}

function App() {
  const [count, setCount] = useState(0);

  useEffect(() => {
    // FIXME: this causes an infinite loop
    setCount(count + 1);
  });

  return (
    <div className="App">
      <h1>Hello World</h1>
      <Chart type="bar" data={{}} />
    </div>
  );
}

export default App;
