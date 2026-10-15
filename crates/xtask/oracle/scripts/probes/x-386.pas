{ parity probe: x-386 }
unit P_x_386;

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
  v := TList.Create; v.Add(5); v.Free; AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
