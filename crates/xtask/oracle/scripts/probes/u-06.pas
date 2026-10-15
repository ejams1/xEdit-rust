{ parity probe: u-06 }
unit P_u_06;

function Initialize: integer;
begin
  AddMessage('len ' + IntToStr(Length('héllo')));
  AddMessage('ord ' + IntToStr(Ord('é')));
end;
end.
