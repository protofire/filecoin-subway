import http from 'k6/http';
import { check } from 'k6';

export const options = {
  vus: 230,
  duration: '90s',
};

const endpointUrl = "http://localhost:8545/";
const headers = {
  "Content-Type": "application/json"
};

const requestOptions = {
  "headers": headers
};

function makeRpcRequest(body) {
  return http.post(endpointUrl, JSON.stringify(body), requestOptions);
}

function getChainHead() {
  const body = { "jsonrpc": "2.0", "id": 1, "method": "Filecoin.ChainHead", "params": [] };
  return makeRpcRequest(body);
}

export default function () {
  const chainHeadRes = getChainHead();
  check(chainHeadRes, {
    "is status 200": (r) => r.status === 200
  })
}
