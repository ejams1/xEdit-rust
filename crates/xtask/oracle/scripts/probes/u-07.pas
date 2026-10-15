{ parity probe: u-07 }
unit P_u_07;

function Initialize: integer;
begin
  AddMessage('len ' + IntToStr(Length('héllo')));
  AddMessage('ord ' + IntToStr(Ord('é')));
end;
end.
