unit P_t_19;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
var
  i: Integer;
begin
end;

function Finalize: integer;
begin
  AddMessage('fin');
end;

end.
