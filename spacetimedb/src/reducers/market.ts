import { t, SenderError } from 'spacetimedb/server';
import { marketSale, type Tradeable } from '../../../shared/economy.ts';
import { ResourceType } from '../../../shared/enums.ts';
import { spacetimedb } from '../schema/db.ts';

// Sell wood or stone for gold at MARKET_RATE. `resType` selects which raw
// resource to sell (Wood or Stone only — Food/Gold are not tradeable here).
// `amount` is the quantity offered; the sale rounds down to whole lots so a coin
// is never minted for free. Authority is ctx.sender — the seller spends their own
// stockpile only.
export const marketTrade = spacetimedb.reducer(
  { resType: t.u8(), amount: t.u32() },
  (ctx, { resType, amount }) => {
    const p = ctx.db.player.identity.find(ctx.sender);
    if (!p) throw new SenderError('not in game');
    const field: Tradeable | null =
      resType === ResourceType.Wood
        ? 'wood'
        : resType === ResourceType.Stone
          ? 'stone'
          : null;
    if (!field) throw new SenderError('only wood or stone can be sold');

    const sale = marketSale(p[field], amount);
    if (!sale.ok) throw new SenderError('not enough to trade');

    ctx.db.player.identity.update({
      ...p,
      [field]: p[field] - sale.spent,
      gold: p.gold + sale.gold,
    });
  }
);
