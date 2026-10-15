unit P_t_28;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process: integer;
begin
  AddMessage('noparam');
end;

end.
