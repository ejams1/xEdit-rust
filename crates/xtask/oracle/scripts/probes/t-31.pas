unit P_t_31;

function Initialize: integer;
begin
  AddMessage('init');
  ScriptProcessElements := [etFile];
end;

function Process(e: IInterface): integer;
begin
  AddMessage(Name(e));
end;

end.
