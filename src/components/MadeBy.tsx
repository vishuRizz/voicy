import React from 'react';
import { invoke } from '@tauri-apps/api/core';

export const MadeBy: React.FC = () => (
  <a
    className="made-by"
    href="https://vishu.app"
    onClick={(e) => {
      e.preventDefault();
      invoke('open_portfolio');
    }}
  >
    made via <span>vishu</span>
  </a>
);
