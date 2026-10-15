unit P_t_04;

function Initialize: integer;
begin
  AddMessage('a');
end;

function Finalize: integer;
begin
  AddMessage('b');
end.
