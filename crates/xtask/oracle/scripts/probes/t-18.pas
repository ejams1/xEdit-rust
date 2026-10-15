unit P_t_18;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  AddMessage('p');
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
