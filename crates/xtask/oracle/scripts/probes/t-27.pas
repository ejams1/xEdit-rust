unit P_t_27;

function Initialize: integer;
begin
  AddMessage('init');
end;

procedure Process(e: IInterface);
begin
  AddMessage('proc');
end;

end.
