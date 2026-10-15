unit P_t_30;

function Initialize: integer;
begin
  AddMessage('init');
end;

function Process(e: IInterface): integer;
begin
  AddMessage(Name(e));
end;

end.
