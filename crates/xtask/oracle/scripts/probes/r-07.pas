{ parity probe: r-07 }
unit P_r_07;

function Initialize: integer;
begin
  AddMessage('x ' + IntToStr(TPoint.X));
end;
end.
