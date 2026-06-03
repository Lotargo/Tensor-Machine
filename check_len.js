const http = require('http');

const data = JSON.stringify({
  limit: 100,
  with_vector: true
});

const req = http.request({
  hostname: 'localhost',
  port: 6333,
  path: '/collections/default/points/scroll',
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
    'Content-Length': data.length
  }
}, (res) => {
  let body = '';
  res.on('data', (chunk) => body += chunk);
  res.on('end', () => {
    const obj = JSON.parse(body);
    if (obj.result && obj.result.points) {
      console.log("Total points retrieved:", obj.result.points.length);
      const lengths = {};
      obj.result.points.forEach(p => {
        const len = p.vector ? p.vector.length : 0;
        lengths[len] = (lengths[len] || 0) + 1;
      });
      console.log("Vector lengths distribution:", lengths);
    } else {
      console.log("No points found in default collection.");
    }
  });
});

req.write(data);
req.end();
