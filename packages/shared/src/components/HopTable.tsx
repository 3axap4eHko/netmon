import React from 'react';
import type { HopStats } from '../types';

interface Props {
  hops: HopStats[];
}

function lossClass(count: number): string {
  if (count === 0) return 'loss-good';
  if (count < 3) return 'loss-warn';
  return 'loss-bad';
}

function formatMs(ms: number): string {
  if (ms <= 0) return '-';
  if (ms < 1) return '<1';
  return ms.toFixed(1);
}

export const HopTable = React.memo(function HopTable({ hops }: Props) {
  return (
    <div className="hop-table-container">
      <table className="hop-table">
        <thead>
          <tr>
            <th>#</th>
            <th>Host</th>
            <th>Loss</th>
            <th>Sent</th>
            <th>Recv</th>
            <th>Best</th>
            <th>Avg</th>
            <th>Worst</th>
            <th>Last</th>
          </tr>
        </thead>
        <tbody>
          {hops.map(hop => {
            const lossCount = Math.max(0, hop.sent - hop.recv);
            return (
            <tr key={`${hop.hop}-${hop.ip}`}>
              <td>{hop.hop}</td>
              <td>
                {hop.ip === '*' ? (
                  <span className="hop-timeout">* * *</span>
                ) : (
                  <>
                    {hop.hostname && <span className="hop-hostname">{hop.hostname}</span>}
                    <span className="hop-ip">{hop.hostname ? hop.ip : hop.ip}</span>
                  </>
                )}
              </td>
              <td className={`loss-cell ${lossClass(lossCount)}`}>
                {lossCount}
              </td>
              <td>{hop.sent}</td>
              <td>{hop.recv}</td>
              <td>{formatMs(hop.best)}</td>
              <td>{formatMs(hop.avg)}</td>
              <td>{formatMs(hop.worst)}</td>
              <td>
                {hop.last === -1 ? (
                  <span className="loss-bad">timeout</span>
                ) : (
                  formatMs(hop.last)
                )}
              </td>
            </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
});
