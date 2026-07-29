const state = {
  nextBefore: null,
  loadedBlocks: [],
  refreshing: false,
  refreshTimer: null,
};

const elements = {
  nodePill: document.querySelector('#node-pill'),
  nodePillText: document.querySelector('#node-pill-text'),
  alert: document.querySelector('#connection-alert'),
  alertError: document.querySelector('#connection-error'),
  retry: document.querySelector('#retry-button'),
  searchForm: document.querySelector('#search-form'),
  searchInput: document.querySelector('#search-input'),
  height: document.querySelector('#metric-height'),
  tip: document.querySelector('#metric-tip'),
  transactions: document.querySelector('#metric-transactions'),
  mempoolMetric: document.querySelector('#metric-mempool'),
  mining: document.querySelector('#metric-mining'),
  difficulty: document.querySelector('#metric-difficulty'),
  reward: document.querySelector('#metric-reward'),
  uptime: document.querySelector('#metric-uptime'),
  blocksBody: document.querySelector('#blocks-body'),
  blocksRange: document.querySelector('#blocks-range'),
  loadMore: document.querySelector('#load-more-button'),
  mempoolCount: document.querySelector('#mempool-count'),
  mempoolList: document.querySelector('#mempool-list'),
  networkId: document.querySelector('#network-id'),
  nodeVersion: document.querySelector('#node-version'),
  databasePath: document.querySelector('#database-path'),
  lastUpdate: document.querySelector('#last-update'),
  explorerVersion: document.querySelector('#explorer-version'),
  dialog: document.querySelector('#details-dialog'),
  dialogClose: document.querySelector('#dialog-close'),
  dialogEyebrow: document.querySelector('#dialog-eyebrow'),
  dialogTitle: document.querySelector('#dialog-title'),
  dialogContent: document.querySelector('#dialog-content'),
  toast: document.querySelector('#toast'),
};

const numberFormat = new Intl.NumberFormat('pt-BR');
const dateFormat = new Intl.DateTimeFormat('pt-BR', {
  dateStyle: 'short',
  timeStyle: 'medium',
});

async function api(path) {
  const response = await fetch(path, {
    headers: { Accept: 'application/json' },
    cache: 'no-store',
  });
  let payload;
  try {
    payload = await response.json();
  } catch {
    throw new Error(`Resposta inválida do explorador (${response.status})`);
  }
  if (!response.ok || !payload.ok) {
    throw new Error(payload.error || `Erro HTTP ${response.status}`);
  }
  return payload.data;
}

function escapeHtml(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#039;');
}

function shortHash(value, left = 10, right = 8) {
  const text = String(value || '');
  if (text.length <= left + right + 1) return text;
  return `${text.slice(0, left)}…${text.slice(-right)}`;
}

function formatInteger(value) {
  try {
    return numberFormat.format(BigInt(String(value)));
  } catch {
    return String(value ?? '—');
  }
}

function formatWyn(value) {
  const source = String(value ?? '0.00000000');
  const negative = source.startsWith('-');
  const normalized = negative ? source.slice(1) : source;
  const [whole = '0', fraction = ''] = normalized.split('.');
  let formattedWhole;
  try {
    formattedWhole = numberFormat.format(BigInt(whole || '0'));
  } catch {
    formattedWhole = whole || '0';
  }
  const padded = fraction.padEnd(8, '0').slice(0, 8);
  return `${negative ? '-' : ''}${formattedWhole},${padded} WYN`;
}

function formatDate(timestamp) {
  if (!timestamp) return 'Gênesis';
  return dateFormat.format(new Date(Number(timestamp)));
}

function formatRelative(timestamp) {
  if (!timestamp) return 'bloco gênesis';
  const seconds = Math.max(0, Math.floor((Date.now() - Number(timestamp)) / 1000));
  if (seconds < 10) return 'agora';
  if (seconds < 60) return `há ${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `há ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `há ${hours} h`;
  const days = Math.floor(hours / 24);
  return `há ${days} d`;
}

function formatUptime(seconds) {
  const value = Number(seconds || 0);
  const days = Math.floor(value / 86400);
  const hours = Math.floor((value % 86400) / 3600);
  const minutes = Math.floor((value % 3600) / 60);
  if (days) return `${days}d ${hours}h ${minutes}min`;
  if (hours) return `${hours}h ${minutes}min`;
  return `${minutes}min`;
}

function formatBytes(bytes) {
  const value = Number(bytes || 0);
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(2)} MiB`;
}

function showConnected() {
  elements.nodePill.classList.remove('offline');
  elements.nodePillText.textContent = 'Nó conectado';
  elements.alert.hidden = true;
}

function showDisconnected(error) {
  elements.nodePill.classList.add('offline');
  elements.nodePillText.textContent = 'Nó indisponível';
  elements.alert.hidden = false;
  elements.alertError.textContent = error?.message || String(error);
}

function showToast(message) {
  elements.toast.textContent = message;
  elements.toast.hidden = false;
  window.clearTimeout(showToast.timer);
  showToast.timer = window.setTimeout(() => {
    elements.toast.hidden = true;
  }, 2600);
}

async function refreshDashboard({ resetBlocks = true } = {}) {
  if (state.refreshing) return;
  state.refreshing = true;
  try {
    const [status, blocks, mempool] = await Promise.all([
      api('/api/status'),
      api('/api/blocks?limit=20'),
      api('/api/mempool'),
    ]);
    showConnected();
    renderStatus(status);
    if (resetBlocks) {
      state.loadedBlocks = blocks.items;
      state.nextBefore = blocks.next_before;
      renderBlocks(state.loadedBlocks, blocks.total_blocks);
    }
    renderMempool(mempool);
  } catch (error) {
    showDisconnected(error);
  } finally {
    state.refreshing = false;
  }
}

function renderStatus(status) {
  const { node, chain } = status;
  elements.height.textContent = formatInteger(node.height);
  elements.tip.textContent = `Tip ${shortHash(node.tip_hash, 7, 6)}`;
  elements.transactions.textContent = formatInteger(chain.transactions);
  elements.mempoolMetric.textContent = `${formatInteger(node.mempool_size)} no mempool`;
  elements.mining.textContent = node.mining_enabled ? 'Ativa' : 'Pausada';
  elements.mining.style.color = node.mining_enabled ? 'var(--accent-strong)' : 'var(--warning)';
  elements.difficulty.textContent = `Dificuldade ${node.difficulty}`;
  elements.reward.textContent = formatWyn(node.block_reward_wyn);
  elements.uptime.textContent = `Uptime ${formatUptime(node.uptime_seconds)}`;
  elements.networkId.textContent = node.network_id;
  elements.nodeVersion.textContent = `v${node.version}`;
  elements.databasePath.textContent = node.database;
  elements.databasePath.title = node.database;
  elements.lastUpdate.textContent = new Date().toLocaleTimeString('pt-BR');
  elements.explorerVersion.textContent = `v${status.explorer_version}`;
}

function renderBlocks(blocks, totalBlocks) {
  if (!blocks.length) {
    elements.blocksBody.innerHTML = '<tr class="loading-row"><td colspan="6">Nenhum bloco encontrado.</td></tr>';
    elements.blocksRange.textContent = '0 blocos';
    elements.loadMore.disabled = true;
    return;
  }

  elements.blocksBody.innerHTML = blocks.map((block) => `
    <tr>
      <td><button class="block-button" type="button" data-block-height="${block.height}">#${formatInteger(block.height)}</button></td>
      <td><button class="hash-button mono" type="button" data-block-height="${block.height}" title="${escapeHtml(block.hash)}">${escapeHtml(shortHash(block.hash))}</button></td>
      <td title="${escapeHtml(formatDate(block.timestamp))}">${escapeHtml(formatRelative(block.timestamp))}</td>
      <td>${formatInteger(block.transaction_count)}</td>
      <td>${block.miner_address ? `<button class="address-button mono" type="button" data-address="${escapeHtml(block.miner_address)}" title="${escapeHtml(block.miner_address)}">${escapeHtml(shortHash(block.miner_address, 8, 5))}</button>` : '<span class="muted">Gênesis</span>'}</td>
      <td class="reward">${block.reward_wyn ? escapeHtml(formatWyn(block.reward_wyn)) : '—'}</td>
    </tr>
  `).join('');

  const first = blocks[0].height;
  const last = blocks[blocks.length - 1].height;
  elements.blocksRange.textContent = `Blocos #${formatInteger(first)} até #${formatInteger(last)} de ${formatInteger(totalBlocks)}`;
  elements.loadMore.disabled = state.nextBefore === null || state.nextBefore === undefined;
}

function renderMempool(transactions) {
  elements.mempoolCount.textContent = formatInteger(transactions.length);
  if (!transactions.length) {
    elements.mempoolList.innerHTML = `
      <div class="empty-state">
        <span aria-hidden="true">✓</span>
        <strong>Mempool vazio</strong>
        <p>As novas transações aparecerão aqui antes da confirmação.</p>
      </div>
    `;
    return;
  }

  elements.mempoolList.innerHTML = transactions.map((tx) => `
    <article class="mempool-item">
      <div class="mempool-item-head">
        <button class="tx-button mono" type="button" data-txid="${escapeHtml(tx.id)}" title="${escapeHtml(tx.id)}">${escapeHtml(shortHash(tx.id, 9, 7))}</button>
        <span class="pending-badge">pendente</span>
      </div>
      <div class="mempool-item-meta">
        <span>${formatInteger(tx.input_count)} input(s) · ${formatInteger(tx.output_count)} output(s)</span>
        <strong class="reward">${escapeHtml(formatWyn(tx.total_output_wyn))}</strong>
      </div>
    </article>
  `).join('');
}

async function loadMoreBlocks() {
  if (state.nextBefore === null || state.nextBefore === undefined) return;
  elements.loadMore.disabled = true;
  elements.loadMore.textContent = 'Carregando…';
  try {
    const page = await api(`/api/blocks?limit=20&before=${encodeURIComponent(state.nextBefore)}`);
    state.loadedBlocks = [...state.loadedBlocks, ...page.items];
    state.nextBefore = page.next_before;
    renderBlocks(state.loadedBlocks, page.total_blocks);
  } catch (error) {
    showToast(error.message);
  } finally {
    elements.loadMore.textContent = 'Carregar anteriores';
    elements.loadMore.disabled = state.nextBefore === null || state.nextBefore === undefined;
  }
}

async function openBlock(height) {
  openLoading('BLOCO', `Bloco #${formatInteger(height)}`);
  try {
    const block = await api(`/api/block/${encodeURIComponent(height)}`);
    renderBlockDetails(block);
  } catch (error) {
    renderDialogError(error);
  }
}

async function openTransaction(transactionId) {
  openLoading('TRANSAÇÃO', shortHash(transactionId, 15, 12));
  try {
    const location = await api(`/api/transaction/${encodeURIComponent(transactionId)}`);
    renderTransactionLocation(location);
  } catch (error) {
    renderDialogError(error);
  }
}

async function openAddress(address) {
  openLoading('ENDEREÇO', shortHash(address, 14, 10));
  try {
    const data = await api(`/api/address/${encodeURIComponent(address)}`);
    renderAddressDetails(data);
  } catch (error) {
    renderDialogError(error);
  }
}

async function runSearch(term) {
  openLoading('BUSCA', shortHash(term, 18, 12));
  try {
    const result = await api(`/api/search?q=${encodeURIComponent(term)}`);
    if (result.kind === 'block') renderBlockDetails(result.data);
    else if (result.kind === 'transaction') renderTransactionLocation(result.data);
    else if (result.kind === 'address') renderAddressDetails(result.data);
    else throw new Error('Tipo de resultado desconhecido');
  } catch (error) {
    renderDialogError(error);
  }
}

function openLoading(eyebrow, title) {
  elements.dialogEyebrow.textContent = eyebrow;
  elements.dialogTitle.textContent = title;
  elements.dialogContent.innerHTML = '<div class="empty-state"><span aria-hidden="true">…</span><strong>Consultando a blockchain</strong></div>';
  if (!elements.dialog.open) elements.dialog.showModal();
}

function renderDialogError(error) {
  elements.dialogEyebrow.textContent = 'NÃO ENCONTRADO';
  elements.dialogTitle.textContent = 'A consulta falhou';
  elements.dialogContent.innerHTML = `
    <div class="empty-state">
      <span aria-hidden="true">!</span>
      <strong>${escapeHtml(error.message)}</strong>
      <p>Confira o valor informado e se o wyncoind está em execução.</p>
    </div>
  `;
}

function renderBlockDetails(block) {
  const summary = block.summary;
  elements.dialogEyebrow.textContent = 'BLOCO CONFIRMADO';
  elements.dialogTitle.textContent = `Bloco #${formatInteger(summary.height)}`;
  elements.dialogContent.innerHTML = `
    <div class="detail-grid">
      ${detailStat('Confirmações', formatInteger(block.confirmations))}
      ${detailStat('Transações', formatInteger(summary.transaction_count))}
      ${detailStat('Tamanho', formatBytes(summary.size_bytes))}
      ${detailStat('Dificuldade', formatInteger(summary.difficulty))}
      ${detailStat('Nonce', formatInteger(summary.nonce))}
      ${detailStat('Total de outputs', formatWyn(summary.total_output_wyn))}
    </div>
    ${dataRow('Hash', copyable(summary.hash))}
    ${dataRow('Hash anterior', summary.height === 0 ? '<span class="muted">Não existe — bloco gênesis</span>' : `<button class="hash-button mono" type="button" data-block-height="${summary.height - 1}">${escapeHtml(summary.previous_hash)}</button>`)}
    ${dataRow('Merkle root', copyable(summary.merkle_root))}
    ${dataRow('Data e hora', escapeHtml(formatDate(summary.timestamp)))}
    ${dataRow('Minerador', summary.miner_address ? `<button class="address-button mono" type="button" data-address="${escapeHtml(summary.miner_address)}">${escapeHtml(summary.miner_address)}</button>` : '<span class="muted">Bloco gênesis</span>')}
    ${dataRow('Recompensa', summary.reward_wyn ? `<span class="reward">${escapeHtml(formatWyn(summary.reward_wyn))}</span>` : '—')}
    <h3 class="section-title">Transações (${formatInteger(block.transactions.length)})</h3>
    ${block.transactions.length ? block.transactions.map(transactionCard).join('') : '<div class="empty-state"><strong>Nenhuma transação neste bloco.</strong></div>'}
  `;
}

function renderTransactionLocation(location) {
  const tx = location.transaction;
  elements.dialogEyebrow.textContent = location.in_mempool ? 'TRANSAÇÃO PENDENTE' : 'TRANSAÇÃO CONFIRMADA';
  elements.dialogTitle.textContent = shortHash(tx.id, 18, 14);
  elements.dialogContent.innerHTML = `
    <div class="detail-grid">
      ${detailStat('Status', location.in_mempool ? 'No mempool' : `${formatInteger(tx.confirmations)} confirmação(ões)`)}
      ${detailStat('Tipo', tx.is_coinbase ? 'Coinbase' : 'Regular')}
      ${detailStat('Valor total', formatWyn(tx.total_output_wyn))}
      ${detailStat('Inputs', formatInteger(tx.input_count))}
      ${detailStat('Outputs', formatInteger(tx.output_count))}
      ${detailStat('Bloco', tx.block_height === null ? 'Pendente' : `#${formatInteger(tx.block_height)}`)}
    </div>
    ${dataRow('TXID', copyable(tx.id))}
    ${dataRow('Data e hora', escapeHtml(formatDate(tx.timestamp)))}
    ${tx.coinbase_data ? dataRow('Dados coinbase', `<span class="mono">${escapeHtml(tx.coinbase_data)}</span>`) : ''}
    ${renderInputsOutputs(tx)}
    ${location.block ? `<h3 class="section-title">Incluída no bloco</h3>${blockMiniCard(location.block)}` : ''}
  `;
}

function renderAddressDetails(address) {
  elements.dialogEyebrow.textContent = 'ENDEREÇO WYNCOIN';
  elements.dialogTitle.textContent = shortHash(address.address, 18, 14);
  elements.dialogContent.innerHTML = `
    <div class="detail-grid">
      ${detailStat('Saldo confirmado', formatWyn(address.confirmed_balance_wyn))}
      ${detailStat('Total recebido', formatWyn(address.received_wyn))}
      ${detailStat('Total enviado', formatWyn(address.sent_wyn))}
    </div>
    ${dataRow('Endereço', copyable(address.address))}
    <h3 class="section-title">Atividade (${formatInteger(address.activity.length)})</h3>
    ${address.activity.length ? address.activity.map(activityCard).join('') : '<div class="empty-state"><strong>Nenhuma atividade encontrada.</strong></div>'}
  `;
}

function transactionCard(tx) {
  return `
    <article class="transaction-card">
      <div class="transaction-head">
        <button class="tx-button mono" type="button" data-txid="${escapeHtml(tx.id)}" title="${escapeHtml(tx.id)}"><strong>${escapeHtml(shortHash(tx.id, 15, 12))}</strong></button>
        <span class="type-badge">${tx.is_coinbase ? 'coinbase' : 'regular'}</span>
      </div>
      <div class="transaction-foot">
        <span>${formatInteger(tx.input_count)} input(s) · ${formatInteger(tx.output_count)} output(s)</span>
        <strong class="reward">${escapeHtml(formatWyn(tx.total_output_wyn))}</strong>
      </div>
    </article>
  `;
}

function blockMiniCard(block) {
  return `
    <article class="transaction-card">
      <div class="transaction-head">
        <button class="block-button" type="button" data-block-height="${block.height}"><strong>Bloco #${formatInteger(block.height)}</strong></button>
        <span class="type-badge">${formatInteger(block.transaction_count)} TXs</span>
      </div>
      <div class="transaction-foot mono">
        <span>${escapeHtml(shortHash(block.hash, 14, 12))}</span>
        <span>${escapeHtml(formatDate(block.timestamp))}</span>
      </div>
    </article>
  `;
}

function renderInputsOutputs(tx) {
  const inputs = tx.inputs.length
    ? tx.inputs.map((input) => `
        <div class="io-item">
          <button class="tx-button mono" type="button" data-txid="${escapeHtml(input.transaction_id)}">${escapeHtml(shortHash(input.transaction_id, 10, 8))}:${input.output_index}</button>
          <span class="muted">Assinatura ${escapeHtml(shortHash(input.signature, 8, 6))}</span>
        </div>
      `).join('')
    : '<div class="io-item muted">Sem inputs (coinbase)</div>';

  const outputs = tx.outputs.length
    ? tx.outputs.map((output) => `
        <div class="io-item">
          <button class="address-button mono" type="button" data-address="${escapeHtml(output.recipient)}">${escapeHtml(shortHash(output.recipient, 10, 7))}</button>
          <span class="io-amount">#${output.index} · ${escapeHtml(formatWyn(output.amount_wyn))}</span>
        </div>
      `).join('')
    : '<div class="io-item muted">Sem outputs</div>';

  return `
    <div class="io-columns">
      <section class="io-box"><h4>Inputs</h4>${inputs}</section>
      <section class="io-box"><h4>Outputs</h4>${outputs}</section>
    </div>
  `;
}

function activityCard(activity) {
  const netValue = String(activity.net_wyn || '0');
  const negative = netValue.startsWith('-');
  const neutral = /^-?0\.0+$/.test(netValue);
  const netClass = neutral ? 'muted' : negative ? 'net-negative' : 'net-positive';
  const prefix = !negative && !neutral ? '+' : '';
  return `
    <article class="activity-card">
      <div class="activity-head">
        <button class="tx-button mono" type="button" data-txid="${escapeHtml(activity.transaction_id)}"><strong>${escapeHtml(shortHash(activity.transaction_id, 14, 11))}</strong></button>
        <strong class="${netClass}">${prefix}${escapeHtml(formatWyn(activity.net_wyn))}</strong>
      </div>
      <div class="transaction-foot">
        <span>${activity.block_height === null ? 'Mempool' : `Bloco #${formatInteger(activity.block_height)}`} · ${escapeHtml(formatDate(activity.timestamp))}</span>
        <span>${activity.is_coinbase ? 'Coinbase' : `${formatInteger(activity.confirmations)} conf.`}</span>
      </div>
    </article>
  `;
}

function detailStat(label, value) {
  return `<div class="detail-stat"><span>${escapeHtml(label)}</span><strong>${escapeHtml(value)}</strong></div>`;
}

function dataRow(label, valueHtml) {
  return `<div class="data-row"><span>${escapeHtml(label)}</span><div class="data-value">${valueHtml}</div></div>`;
}

function copyable(value) {
  const escaped = escapeHtml(value);
  return `<span class="mono">${escaped}</span><button class="copy-button" type="button" data-copy="${escaped}">copiar</button>`;
}

async function copyValue(value) {
  try {
    await navigator.clipboard.writeText(value);
    showToast('Copiado para a área de transferência.');
  } catch {
    showToast('Não foi possível copiar automaticamente.');
  }
}

function handleActionClick(event) {
  const blockButton = event.target.closest('[data-block-height]');
  if (blockButton) {
    openBlock(blockButton.dataset.blockHeight);
    return;
  }
  const txButton = event.target.closest('[data-txid]');
  if (txButton) {
    openTransaction(txButton.dataset.txid);
    return;
  }
  const addressButton = event.target.closest('[data-address]');
  if (addressButton) {
    openAddress(addressButton.dataset.address);
    return;
  }
  const copyButton = event.target.closest('[data-copy]');
  if (copyButton) copyValue(copyButton.dataset.copy);
}

elements.searchForm.addEventListener('submit', (event) => {
  event.preventDefault();
  const term = elements.searchInput.value.trim();
  if (term) runSearch(term);
});

document.querySelectorAll('.example-search').forEach((button) => {
  button.addEventListener('click', () => {
    elements.searchInput.value = button.dataset.query;
    runSearch(button.dataset.query);
  });
});

elements.retry.addEventListener('click', () => refreshDashboard());
elements.loadMore.addEventListener('click', loadMoreBlocks);
elements.dialogClose.addEventListener('click', () => elements.dialog.close());
elements.dialog.addEventListener('click', (event) => {
  if (event.target === elements.dialog) elements.dialog.close();
});
document.addEventListener('click', handleActionClick);

refreshDashboard();
state.refreshTimer = window.setInterval(() => refreshDashboard(), 10_000);
