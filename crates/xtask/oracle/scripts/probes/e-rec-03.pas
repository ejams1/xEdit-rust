{ parity probe: e-rec-03 }
unit P_e_rec_03;

function Down(n: Integer): Integer;
begin
  if n = 0 then Result := 0 else Result := 1 + Down(n - 1);
end;

function Initialize: integer;
begin
  AddMessage('depth ' + IntToStr(Down(3000)));
end;
end.
