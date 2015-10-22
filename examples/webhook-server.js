'use strict';

const http = require('http');
const https = require('https');
const querystring = require('querystring');
const urilib = require('url');
const Buffer = require('buffer').Buffer;

function processMessage(buffer) {
  let message;
  try {
    message = JSON.parse(buffer);
  } catch (e) {
    return console.log("could not parse message");
  }

  const fullyQualifiedBranch = `${message.owner}.${message.repo}.${message.branch}:${message.task_id.slice(0, 6)}`;
  const messageMap = new Map();
  messageMap.set('started', `[\`${fullyQualifiedBranch}\`] 📦 Starting build...`);
  messageMap.set('success', `[\`${fullyQualifiedBranch}\`] 🎊 Success!`);
  messageMap.set('failed', `[\`${fullyQualifiedBranch}\`] 🚨 Build failed, see \`${message.job_url}\` for details`);

  const status = message.status.toLowerCase();
  const url = process.env.SLACK_URL;
  const payload = {
    channel: '#bocoupcom',
    username: 'hookshotbot',
    text: messageMap.get(status),
    icon_emoji: ':shipit:',
  };

  const postData = querystring.stringify({
    payload: JSON.stringify(payload),
  });

  const request = https.request(Object.assign(urilib.parse(url), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      'Content-Length': postData.length,
    },
  }), (response) => {
    console.log('STATUS: ' + response.statusCode);
    console.log('HEADERS: ' + JSON.stringify(response.headers));
    response.setEncoding('utf8');
    response.pipe(process.stdout, {end: false});
  });
  request.write(postData);
  request.end();

}

const server = http.createServer((req, res) => {
  process.stdout.write(JSON.stringify(req.headers));
  process.stdout.write('\n');

  let buffer = '';
  req.on('data', (incoming) => buffer += incoming);
  req.once('end', () => {
    console.log(buffer);
    processMessage(buffer);
  });

  res.end('done');
});

server.listen(5600, () => console.error('listening on %s', server.address().port));
