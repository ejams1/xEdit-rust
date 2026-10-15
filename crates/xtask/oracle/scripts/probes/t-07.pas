unit P_t_07;

function Initialize: integer;
begin
  AddMessage('a');
end;

end.

function Finalize: integer;
begin
  AddMessage('b');
end;
