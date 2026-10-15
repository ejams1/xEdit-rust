{ parity probe: x-322 }
unit P_x_322;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  sl.Destroy; AddMessage('ok');
  AddMessage('after');
end;
end.
