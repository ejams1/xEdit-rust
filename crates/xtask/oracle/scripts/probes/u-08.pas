{ parity probe: u-08 }
unit P_u_08;

function Initialize: integer;
begin
  AddMessage('len ' + IntToStr(Length('héllo')));
end;
end.
